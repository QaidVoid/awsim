//! Run Command executed for real through the agents on EC2 instances.
//!
//! Used for `AWS-RunShellScript` when EC2 runs its instances as virtual
//! machines. Every other case keeps the synthetic path in `commands`.

use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, InstanceAgents, ManagedInstance, RequestContext};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::state::{Command, Invocation, SsmState};

/// The document this path runs.
pub const SHELL_DOCUMENT: &str = "AWS-RunShellScript";
/// `GetCommandInvocation` keeps the first this many characters of output.
const STDOUT_LIMIT: usize = 24_000;
const STDERR_LIMIT: usize = 8_000;
/// `ListCommandInvocations` plugin output keeps this many.
const PLUGIN_OUTPUT_LIMIT: usize = 2_500;
const DEFAULT_EXECUTION_TIMEOUT: u64 = 3_600;

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn iso(millis: u64) -> String {
    let secs = millis / 1000;
    let days = secs / 86_400;
    let rem = secs % 86_400;
    // Civil date from days since 1970-01-01, after Howard Hinnant.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        millis % 1000
    )
}

fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn invalid_instance(ids: &[String], account: &str) -> AwsError {
    AwsError::bad_request(
        "InvalidInstanceId",
        format!(
            "Instances [[{}]] not in a valid state for account {account}",
            ids.join(", ")
        ),
    )
}

/// The instance ids a `SendCommand` names, from `InstanceIds` and any
/// `InstanceIds` target.
pub fn target_instance_ids(input: &Value) -> Vec<String> {
    let mut ids: Vec<String> = input["InstanceIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    for target in input["Targets"].as_array().into_iter().flatten() {
        if target["Key"]
            .as_str()
            .is_some_and(|k| k.eq_ignore_ascii_case("instanceids"))
        {
            ids.extend(
                target["Values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(String::from)),
            );
        }
    }
    ids
}

fn first_param<'a>(parameters: &'a Value, name: &str) -> Option<&'a str> {
    match &parameters[name] {
        Value::Array(values) => values.first().and_then(Value::as_str),
        Value::String(value) => Some(value),
        _ => None,
    }
}

/// The script an `AWS-RunShellScript` command runs: its `commands`
/// joined by newlines, in `workingDirectory` when one is given.
fn script_of(parameters: &Value) -> Result<String, AwsError> {
    let commands: Vec<&str> = match &parameters["commands"] {
        Value::Array(lines) => lines.iter().filter_map(Value::as_str).collect(),
        Value::String(line) => vec![line.as_str()],
        _ => Vec::new(),
    };
    if commands.is_empty() {
        return Err(AwsError::bad_request(
            "InvalidParameters",
            "Parameters required: commands",
        ));
    }
    let mut script = String::new();
    if let Some(dir) = first_param(parameters, "workingDirectory").filter(|d| !d.is_empty()) {
        script.push_str(&format!("cd '{}' || exit 1\n", dir.replace('\'', "'\\''")));
    }
    script.push_str(&commands.join("\n"));
    script.push('\n');
    Ok(script)
}

/// `SendCommand` for `AWS-RunShellScript`, started on every target
/// instance. Every target must have an online agent, or nothing runs.
pub async fn send_command(
    state: &SsmState,
    input: &Value,
    ctx: &RequestContext,
    agents: &dyn InstanceAgents,
) -> Result<Value, AwsError> {
    let ids = target_instance_ids(input);
    let parameters = input["Parameters"].clone();
    let script = script_of(&parameters)?;
    let timeout_secs = first_param(&parameters, "executionTimeout")
        .and_then(|t| t.parse().ok())
        .unwrap_or(DEFAULT_EXECUTION_TIMEOUT);

    let online: Vec<String> = agents
        .managed_instances(&ctx.account_id, &ctx.region)
        .into_iter()
        .filter(|m| m.ping_status == "Online")
        .map(|m| m.instance_id)
        .collect();
    let offline: Vec<String> = ids
        .iter()
        .filter(|id| !online.contains(id))
        .cloned()
        .collect();
    if !offline.is_empty() {
        return Err(invalid_instance(&offline, &ctx.account_id));
    }

    let started = now_millis();
    let mut invocations = Vec::with_capacity(ids.len());
    for id in &ids {
        let handle = agents
            .run_shell(&ctx.account_id, &ctx.region, id, &script)
            .await?;
        invocations.push(Invocation {
            instance_id: id.clone(),
            handle,
            status: "InProgress".to_string(),
            response_code: -1,
            std_out: String::new(),
            std_err: String::new(),
            started,
            ended: None,
            timeout_secs,
        });
    }

    let command_id = Uuid::new_v4().to_string();
    info!(command_id = %command_id, instances = ?ids, "SendCommand");
    let targets = input["Targets"].as_array().cloned().unwrap_or_default();
    state.commands.insert(
        command_id.clone(),
        Command {
            command_id: command_id.clone(),
            document_name: SHELL_DOCUMENT.to_string(),
            targets: targets.clone(),
            instance_ids: ids.clone(),
            status: "InProgress".to_string(),
            created_time: started / 1000,
            std_out: String::new(),
            std_err: String::new(),
            invocations,
        },
    );

    Ok(json!({
        "Command": {
            "CommandId": command_id,
            "DocumentName": SHELL_DOCUMENT,
            "DocumentVersion": "$DEFAULT",
            "InstanceIds": ids,
            "Targets": targets,
            "Parameters": parameters,
            "Comment": input["Comment"].as_str().unwrap_or_default(),
            "Status": "Pending",
            "StatusDetails": "Pending",
            "RequestedDateTime": started as f64 / 1000.0,
            "TargetCount": ids.len(),
            "CompletedCount": 0,
            "ErrorCount": 0,
            "DeliveryTimedOutCount": 0,
        }
    }))
}

/// Read the exit of every unfinished invocation of a command, optionally
/// only on one instance, and store what finished.
pub async fn settle(
    state: &SsmState,
    command_id: &str,
    instance_id: Option<&str>,
    ctx: &RequestContext,
    agents: &dyn InstanceAgents,
) {
    let pending: Vec<Invocation> = match state.commands.get(command_id) {
        Some(command) => command
            .invocations
            .iter()
            .filter(|i| i.status == "InProgress")
            .filter(|i| instance_id.is_none_or(|id| id == i.instance_id))
            .cloned()
            .collect(),
        None => return,
    };
    for invocation in pending {
        let outcome = agents
            .command_exit(
                &ctx.account_id,
                &ctx.region,
                &invocation.instance_id,
                invocation.handle,
            )
            .await;
        let now = now_millis();
        let Some(mut command) = state.commands.get_mut(command_id) else {
            return;
        };
        let Some(stored) = command
            .invocations
            .iter_mut()
            .find(|i| i.instance_id == invocation.instance_id && i.status == "InProgress")
        else {
            continue;
        };
        match outcome {
            Ok(Some(exit)) => {
                stored.status = if exit.exit_code == 0 {
                    "Success"
                } else {
                    "Failed"
                }
                .to_string();
                stored.response_code = exit.exit_code;
                stored.std_out = exit.stdout;
                stored.std_err = exit.stderr;
                stored.ended = Some(now);
            }
            // The agent no longer knows the process, which happens when
            // another reader collected its exit first. That reader stores
            // the result, so this one leaves it alone.
            Ok(None) | Err(_) => {
                if now.saturating_sub(stored.started) > stored.timeout_secs * 1000 {
                    stored.status = "TimedOut".to_string();
                    stored.ended = Some(now);
                }
            }
        }
        command.status = overall_status(&command.invocations);
    }
}

fn overall_status(invocations: &[Invocation]) -> String {
    if invocations.iter().any(|i| i.status == "InProgress") {
        "InProgress"
    } else if invocations.iter().all(|i| i.status == "Success") {
        "Success"
    } else if invocations.iter().any(|i| i.status == "TimedOut") {
        "TimedOut"
    } else {
        "Failed"
    }
    .to_string()
}

fn elapsed(invocation: &Invocation) -> String {
    let end = invocation.ended.unwrap_or_else(now_millis);
    let ms = end.saturating_sub(invocation.started);
    format!("PT{}.{:03}S", ms / 1000, ms % 1000)
}

/// `GetCommandInvocation` for a command that ran for real.
pub fn get_invocation(command: &Command, instance_id: Option<&str>) -> Result<Value, AwsError> {
    let invocation = command
        .invocations
        .iter()
        .find(|i| instance_id.is_none_or(|id| id == i.instance_id))
        .ok_or_else(|| {
            AwsError::bad_request(
                "InvocationDoesNotExist",
                "An error occurred (InvocationDoesNotExist) when calling the \
                 GetCommandInvocation operation",
            )
        })?;
    Ok(json!({
        "CommandId": command.command_id,
        "InstanceId": invocation.instance_id,
        "Comment": "",
        "DocumentName": command.document_name,
        "DocumentVersion": "$DEFAULT",
        "PluginName": "aws:runShellScript",
        "ResponseCode": invocation.response_code,
        "ExecutionStartDateTime": iso(invocation.started),
        "ExecutionElapsedTime": elapsed(invocation),
        "ExecutionEndDateTime": invocation.ended.map(iso).unwrap_or_default(),
        "Status": invocation.status,
        "StatusDetails": invocation.status,
        "StandardOutputContent": truncate(&invocation.std_out, STDOUT_LIMIT),
        "StandardOutputUrl": "",
        "StandardErrorContent": truncate(&invocation.std_err, STDERR_LIMIT),
        "StandardErrorUrl": "",
        "CloudWatchOutputConfig": {
            "CloudWatchLogGroupName": "",
            "CloudWatchOutputEnabled": false,
        },
    }))
}

/// `ListCommandInvocations` entries for a command that ran for real.
pub fn list_invocations(command: &Command, instance_id: Option<&str>, details: bool) -> Vec<Value> {
    command
        .invocations
        .iter()
        .filter(|i| instance_id.is_none_or(|id| id == i.instance_id))
        .map(|i| {
            let mut item = json!({
                "CommandId": command.command_id,
                "InstanceId": i.instance_id,
                "DocumentName": command.document_name,
                "DocumentVersion": "$DEFAULT",
                "RequestedDateTime": i.started as f64 / 1000.0,
                "Status": i.status,
                "StatusDetails": i.status,
                "StandardOutputUrl": "",
                "StandardErrorUrl": "",
            });
            if details {
                let mut output = i.std_out.clone();
                if !i.std_err.is_empty() {
                    output.push_str("\n----------ERROR-------\n");
                    output.push_str(&i.std_err);
                }
                item["CommandPlugins"] = json!([{
                    "Name": "aws:runShellScript",
                    "Status": i.status,
                    "StatusDetails": i.status,
                    "ResponseCode": i.response_code,
                    "ResponseStartDateTime": i.started as f64 / 1000.0,
                    "ResponseFinishDateTime": i.ended.map(|e| e as f64 / 1000.0),
                    "Output": truncate(&output, PLUGIN_OUTPUT_LIMIT),
                    "StandardOutputUrl": "",
                    "StandardErrorUrl": "",
                }]);
            }
            item
        })
        .collect()
}

/// `DescribeInstanceInformation` entries for instances whose agent
/// answers.
pub fn instance_information(instances: &[ManagedInstance]) -> Vec<Value> {
    instances
        .iter()
        .map(|m| {
            json!({
                "InstanceId": m.instance_id,
                "PingStatus": m.ping_status,
                "LastPingDateTime": m.last_ping,
                "AgentVersion": "3.3.0.0",
                "IsLatestVersion": true,
                "PlatformType": "Linux",
                "PlatformName": m.platform_name,
                "PlatformVersion": m.platform_version,
                "ResourceType": "EC2Instance",
                "IPAddress": m.ip_address,
                "ComputerName": m.computer_name,
                "IamRole": m.iam_role,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_joins_commands_in_working_directory() {
        let script = script_of(&json!({
            "commands": ["echo a", "echo b"],
            "workingDirectory": ["/tmp/it's"],
        }))
        .unwrap();
        assert_eq!(script, "cd '/tmp/it'\\''s' || exit 1\necho a\necho b\n");
        let err = script_of(&json!({})).unwrap_err();
        assert_eq!(err.code, "InvalidParameters");
    }

    #[test]
    fn iso_formats_milliseconds() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_793_208_369_123), "2026-10-28T17:26:09.123Z");
        assert_eq!(iso(951_782_400_000), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn overall_status_prefers_in_progress_then_failure() {
        let mut a = Invocation {
            instance_id: "i-1".into(),
            handle: 1,
            status: "Success".into(),
            response_code: 0,
            std_out: String::new(),
            std_err: String::new(),
            started: 0,
            ended: Some(1),
            timeout_secs: 1,
        };
        let mut b = a.clone();
        assert_eq!(overall_status(&[a.clone(), b.clone()]), "Success");
        b.status = "Failed".into();
        assert_eq!(overall_status(&[a.clone(), b.clone()]), "Failed");
        a.status = "InProgress".into();
        assert_eq!(overall_status(&[a, b]), "InProgress");
    }
}
