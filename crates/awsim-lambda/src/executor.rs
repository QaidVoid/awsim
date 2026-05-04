use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use tracing::{debug, warn};

pub struct ExecutionResult {
    /// HTTP-level status code (200 for success/handled errors, 500 for service errors).
    #[allow(dead_code)]
    pub status_code: u16,
    pub payload: String,
    pub error: Option<String>,
    /// Combined stderr + stdout. Surfaced on Invoke responses when the
    /// caller passes `LogType=Tail` (last 4 KiB, base64-encoded, in the
    /// `X-Amz-Log-Result` header / `LogResult` body field).
    pub logs: String,
}

pub fn execute_function(
    runtime: &str,
    handler: &str,
    code_dir: &Path,
    event_json: &str,
    env_vars: &HashMap<String, String>,
    timeout_secs: u32,
) -> ExecutionResult {
    debug!(runtime, handler, "Executing Lambda function");
    match runtime {
        r if r.starts_with("nodejs") => {
            execute_node(handler, code_dir, event_json, env_vars, timeout_secs)
        }
        r if r.starts_with("python") => {
            execute_python(handler, code_dir, event_json, env_vars, timeout_secs)
        }
        _ => {
            warn!(runtime, "Unsupported runtime");
            ExecutionResult {
                status_code: 200,
                payload: format!(
                    r#"{{"errorMessage":"Unsupported runtime: {}","errorType":"UnsupportedRuntime"}}"#,
                    runtime
                ),
                error: Some("UnsupportedRuntime".to_string()),
                logs: String::new(),
            }
        }
    }
}

fn execute_node(
    handler: &str,
    code_dir: &Path,
    event_json: &str,
    env_vars: &HashMap<String, String>,
    timeout_secs: u32,
) -> ExecutionResult {
    // handler format: "index.handler" → file "index.js", export "handler"
    let parts: Vec<&str> = handler.splitn(2, '.').collect();
    let (module, func) = if parts.len() == 2 {
        (parts[0], parts[1])
    } else {
        ("index", "handler")
    };

    // Exit codes are how we distinguish Handled (callback(err) — function
    // signalled failure cleanly) from Unhandled (uncaught throw / promise
    // rejection — function crashed) for the X-Amz-Function-Error header.
    let bootstrap = format!(
        r#"
const mod = require('./{module}');
const event = JSON.parse(process.argv[1]);
const errOut = (e) => console.error(JSON.stringify({{ errorMessage: e && e.message || String(e), errorType: e && e.name || 'Error' }}));
const context = {{
    functionName: process.env.AWS_LAMBDA_FUNCTION_NAME || 'test',
    functionVersion: '$LATEST',
    invokedFunctionArn: process.env._HANDLER || '',
    memoryLimitInMB: process.env.AWS_LAMBDA_FUNCTION_MEMORY_SIZE || '128',
    awsRequestId: process.env.AWS_REQUEST_ID || 'local',
    logGroupName: '/aws/lambda/' + (process.env.AWS_LAMBDA_FUNCTION_NAME || 'test'),
    logStreamName: 'local',
    getRemainingTimeInMillis: () => {timeout_secs}000,
    done: (err, result) => {{ if (err) console.error(err); else console.log(JSON.stringify(result)); }},
    succeed: (result) => console.log(JSON.stringify(result)),
    fail: (err) => console.error(err),
}};
const callback = (err, result) => {{
    if (err) {{ errOut(err); process.exit(64); }}
    console.log(JSON.stringify(result));
}};
Promise.resolve(mod.{func}(event, context, callback))
    .then(r => {{ if (r !== undefined) console.log(JSON.stringify(r)); }})
    .catch(e => {{ errOut(e); process.exit(1); }});
"#
    );

    let bootstrap_path = code_dir.join("__awsim_bootstrap.js");
    if let Err(e) = std::fs::write(&bootstrap_path, &bootstrap) {
        return ExecutionResult {
            status_code: 500,
            payload: format!(r#"{{"errorMessage":"Failed to write bootstrap: {}"}}"#, e),
            error: Some("ServiceException".to_string()),
            logs: e.to_string(),
        };
    }

    let mut cmd = Command::new("node");
    cmd.arg(&bootstrap_path)
        .arg(event_json)
        .current_dir(code_dir)
        .env(
            "AWS_LAMBDA_FUNCTION_NAME",
            env_vars
                .get("AWS_LAMBDA_FUNCTION_NAME")
                .map(|s| s.as_str())
                .unwrap_or("test"),
        )
        .env(
            "AWS_LAMBDA_FUNCTION_MEMORY_SIZE",
            env_vars
                .get("AWS_LAMBDA_FUNCTION_MEMORY_SIZE")
                .map(|s| s.as_str())
                .unwrap_or("128"),
        )
        .env(
            "AWS_REGION",
            env_vars
                .get("AWS_REGION")
                .map(|s| s.as_str())
                .unwrap_or("us-east-1"),
        )
        .env(
            "AWS_DEFAULT_REGION",
            env_vars
                .get("AWS_REGION")
                .map(|s| s.as_str())
                .unwrap_or("us-east-1"),
        )
        .env("_HANDLER", handler);

    for (k, v) in env_vars {
        cmd.env(k, v);
    }

    run_command(cmd, timeout_secs)
}

fn execute_python(
    handler: &str,
    code_dir: &Path,
    event_json: &str,
    env_vars: &HashMap<String, String>,
    timeout_secs: u32,
) -> ExecutionResult {
    let parts: Vec<&str> = handler.splitn(2, '.').collect();
    let (module, func) = if parts.len() == 2 {
        (parts[0], parts[1])
    } else {
        ("lambda_function", "lambda_handler")
    };

    // Python has no callback API, so a raised exception is always
    // "Unhandled" — surface it as AWS-style error JSON on stderr and
    // exit 1 so the Rust side maps it accordingly.
    let bootstrap = format!(
        r#"
import sys, json, importlib, os, traceback
sys.path.insert(0, os.environ.get('PYTHONPATH', '.'))
event = json.loads(sys.argv[1])
context = type('Context', (), {{
    'function_name': os.environ.get('AWS_LAMBDA_FUNCTION_NAME', 'test'),
    'function_version': '$LATEST',
    'memory_limit_in_mb': int(os.environ.get('AWS_LAMBDA_FUNCTION_MEMORY_SIZE', '128')),
    'aws_request_id': os.environ.get('AWS_REQUEST_ID', 'local'),
    'log_group_name': '/aws/lambda/' + os.environ.get('AWS_LAMBDA_FUNCTION_NAME', 'test'),
    'log_stream_name': 'local',
    'get_remaining_time_in_millis': lambda self: {timeout_secs}000,
}})()
try:
    mod = importlib.import_module('{module}')
    result = mod.{func}(event, context)
    if result is not None:
        print(json.dumps(result))
except BaseException as e:
    print(json.dumps({{
        'errorMessage': str(e),
        'errorType': type(e).__name__,
        'stackTrace': traceback.format_tb(e.__traceback__),
    }}), file=sys.stderr)
    sys.exit(1)
"#
    );

    let bootstrap_path = code_dir.join("__awsim_bootstrap.py");
    if let Err(e) = std::fs::write(&bootstrap_path, &bootstrap) {
        return ExecutionResult {
            status_code: 500,
            payload: format!(r#"{{"errorMessage":"Failed to write bootstrap: {}"}}"#, e),
            error: Some("ServiceException".to_string()),
            logs: e.to_string(),
        };
    }

    let mut cmd = Command::new("python3");
    cmd.arg(&bootstrap_path)
        .arg(event_json)
        .current_dir(code_dir)
        .env("PYTHONPATH", code_dir);

    for (k, v) in env_vars {
        cmd.env(k, v);
    }

    run_command(cmd, timeout_secs)
}

fn run_command(mut cmd: Command, _timeout_secs: u32) -> ExecutionResult {
    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let logs = format!("{}{}", stderr, stdout);

            // Last non-empty line of stdout is the response payload
            let payload = stdout
                .lines()
                .rfind(|l| !l.trim().is_empty())
                .unwrap_or("null")
                .to_string();

            if output.status.success() {
                ExecutionResult {
                    status_code: 200,
                    payload,
                    error: None,
                    logs,
                }
            } else {
                // On failure, last line of stderr is the error payload.
                // Exit code 64 is the bootstrap's signal that the user
                // called callback(err) — i.e. Handled. Anything else
                // (uncaught throw, OOM, native crash) is Unhandled.
                let error_payload = stderr
                    .lines()
                    .rfind(|l| !l.trim().is_empty())
                    .unwrap_or(r#"{"errorMessage":"Function failed","errorType":"Unhandled"}"#)
                    .to_string();
                let kind = if output.status.code() == Some(64) {
                    "Handled"
                } else {
                    "Unhandled"
                };
                ExecutionResult {
                    status_code: 200,
                    payload: error_payload,
                    error: Some(kind.to_string()),
                    logs,
                }
            }
        }
        Err(e) => {
            // Check if the binary is simply not found
            let msg = if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    r#"{{"errorMessage":"Runtime binary not found. Is node/python3 installed? ({})","errorType":"ServiceException"}}"#,
                    e
                )
            } else {
                format!(
                    r#"{{"errorMessage":"Failed to execute runtime: {}","errorType":"ServiceException"}}"#,
                    e
                )
            };
            ExecutionResult {
                status_code: 500,
                payload: msg.clone(),
                error: Some("ServiceException".to_string()),
                logs: msg,
            }
        }
    }
}
