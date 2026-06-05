//! Workspace automation tasks. Run via `cargo xtask <command>`.
//!
//! Commands:
//!   update-models [SERVICE...]   Vendor Smithy models into `models/`.
//!   --all                        With update-models: vendor every known
//!                                service, not just the ones already present.
//!   --ref <REF>                  Upstream git ref to pin (branch/tag/sha).
//!   --list                       Print the service table and exit.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const UPSTREAM: &str = "https://github.com/aws/api-models-aws";
const DEFAULT_REF: &str = "main";

/// (vendored filename stem, candidate upstream service-dir names).
///
/// The vendored file is always written as `models/<stem>.json` so the
/// conformance loader keeps resolving it by the same name. Candidates are
/// tried in order against the upstream `models/<dir>/service/<ver>/` layout;
/// the first that resolves wins. Most services match their own stem, so they
/// list a single candidate. Divergent ones (e.g. `cognito-idp` ->
/// `cognito-identity-provider`) list the real upstream dir, and a couple list
/// fallbacks where the upstream naming is uncertain.
///
/// OpenSearch is intentionally absent: awsim emulates the OpenSearch REST data
/// plane (`_search`, `_reindex`), which has no Smithy model. The `opensearch`
/// model upstream is the control plane (CreateDomain, ...) and would mislead.
const SERVICES: &[(&str, &[&str])] = &[
    // --- currently vendored (the original set) ---
    ("acm", &["acm"]),
    ("appsync", &["appsync"]),
    ("athena", &["athena"]),
    ("batch", &["batch"]),
    ("bedrock", &["bedrock"]),
    ("cloudformation", &["cloudformation"]),
    ("cloudfront", &["cloudfront"]),
    ("cloudtrail", &["cloudtrail"]),
    ("cloudwatch-logs", &["cloudwatch-logs"]),
    ("cognito-identity", &["cognito-identity"]),
    ("cognito-idp", &["cognito-identity-provider"]),
    ("datasync", &["datasync"]),
    ("dynamodb", &["dynamodb"]),
    ("ec2", &["ec2"]),
    ("ecr", &["ecr"]),
    ("ecs", &["ecs"]),
    ("eks", &["eks"]),
    ("elasticloadbalancingv2", &["elastic-load-balancing-v2"]),
    ("eventbridge", &["eventbridge", "events"]),
    ("firehose", &["firehose"]),
    ("glue", &["glue"]),
    ("iam", &["iam"]),
    ("kinesis", &["kinesis"]),
    ("kms", &["kms"]),
    ("lambda", &["lambda"]),
    ("organizations", &["organizations"]),
    ("polly", &["polly"]),
    ("rds", &["rds"]),
    ("route53", &["route-53", "route53"]),
    ("s3", &["s3"]),
    ("scheduler", &["scheduler"]),
    ("secretsmanager", &["secrets-manager", "secretsmanager"]),
    ("sns", &["sns"]),
    ("sqs", &["sqs"]),
    ("ssm", &["ssm"]),
    ("sso-admin", &["sso-admin"]),
    ("stepfunctions", &["sfn", "stepfunctions", "states"]),
    ("sts", &["sts"]),
    ("wafv2", &["wafv2", "waf-v2"]),
    // --- service crates without a vendored model yet (best-effort) ---
    ("apigateway", &["api-gateway"]),
    ("appconfig", &["appconfig"]),
    ("application-autoscaling", &["application-auto-scaling"]),
    ("backup", &["backup"]),
    ("bedrock-runtime", &["bedrock-runtime"]),
    ("billing", &["billing"]),
    ("comprehend", &["comprehend"]),
    ("efs", &["efs", "elastic-file-system"]),
    ("glacier", &["glacier"]),
    ("identitystore", &["identitystore"]),
    ("kendra", &["kendra"]),
    ("memorydb", &["memorydb"]),
    ("mq", &["mq"]),
    ("pinpoint", &["pinpoint"]),
    ("pipes", &["pipes"]),
    // QLDB is deprecated and not published in api-models-aws; this stays a
    // reported miss until/unless a model appears upstream.
    ("qldb", &["qldb", "qldb-session"]),
    ("resourcegroupstagging", &["resource-groups-tagging-api"]),
    ("servicediscovery", &["servicediscovery"]),
    ("transfer", &["transfer"]),
    ("xray", &["xray"]),
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str);
    match cmd {
        Some("update-models") => {
            if let Err(e) = update_models(&args[1..]) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("help") | Some("--help") | Some("-h") | None => print_help(),
        Some(other) => {
            eprintln!("unknown command: {other}\n");
            print_help();
            std::process::exit(2);
        }
    }
}

fn print_help() {
    println!(
        "cargo xtask <command>\n\n\
         commands:\n  \
         update-models [SERVICE...]  vendor Smithy models into models/\n\n\
         update-models flags:\n  \
         --all          vendor every known service (default: refresh existing only)\n  \
         --ref <REF>    upstream git ref to pin (default: {DEFAULT_REF})\n  \
         --list         print the service table and exit\n\n\
         examples:\n  \
         cargo xtask update-models                 # refresh the models already present\n  \
         cargo xtask update-models dynamodb s3     # refresh just these\n  \
         cargo xtask update-models --all           # vendor everything in the table\n  \
         cargo xtask update-models --ref <sha>     # pin a specific upstream commit"
    );
}

struct Options {
    services: Vec<String>,
    all: bool,
    git_ref: String,
    list: bool,
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut opts = Options {
        services: Vec::new(),
        all: false,
        git_ref: DEFAULT_REF.to_string(),
        list: false,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--all" => opts.all = true,
            "--list" => opts.list = true,
            "--ref" => {
                i += 1;
                opts.git_ref = args.get(i).ok_or("--ref requires a value")?.clone();
            }
            s if s.starts_with('-') => return Err(format!("unknown flag: {s}")),
            s => opts.services.push(s.to_string()),
        }
        i += 1;
    }
    Ok(opts)
}

fn update_models(args: &[String]) -> Result<(), String> {
    let opts = parse_options(args)?;
    let table: BTreeMap<&str, &[&str]> = SERVICES.iter().copied().collect();

    if opts.list {
        for (stem, cands) in &table {
            println!("{stem:30} <- {}", cands.join(", "));
        }
        return Ok(());
    }

    let root = workspace_root();
    let models_dir = root.join("models");
    if !models_dir.is_dir() {
        return Err(format!("{} not found", models_dir.display()));
    }

    // Resolve the target stems.
    let targets: Vec<&str> = if !opts.services.is_empty() {
        let mut out = Vec::new();
        for s in &opts.services {
            match table.keys().find(|k| **k == s.as_str()) {
                Some(k) => out.push(*k),
                None => return Err(format!("unknown service '{s}' (see --list)")),
            }
        }
        out
    } else if opts.all {
        table.keys().copied().collect()
    } else {
        // Default: refresh only the models already vendored.
        table
            .keys()
            .copied()
            .filter(|stem| models_dir.join(format!("{stem}.json")).exists())
            .collect()
    };

    if targets.is_empty() {
        println!("nothing to do (no matching vendored models; use --all to add new ones)");
        return Ok(());
    }

    // Sparse-checkout only the candidate dirs we need so the partial clone
    // stays small. A guessed dir that doesn't exist upstream just yields no
    // files and is reported as a miss below.
    let sparse_paths: Vec<String> = targets
        .iter()
        .flat_map(|stem| table[*stem].iter())
        .map(|dir| format!("models/{dir}"))
        .collect();

    let tmp = std::env::temp_dir().join("awsim-xtask-models");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("mkdir {}: {e}", tmp.display()))?;

    println!("cloning {UPSTREAM} @ {} (sparse)...", opts.git_ref);
    sparse_checkout(&tmp, &opts.git_ref, &sparse_paths)?;
    let sha = git(&tmp, &["rev-parse", "HEAD"])?.trim().to_string();
    let commit_date = git(&tmp, &["show", "-s", "--format=%cI", "HEAD"])?
        .trim()
        .to_string();

    let upstream_models = tmp.join("models");
    let mut vendored: Vec<(String, String)> = Vec::new();
    let mut missed: Vec<&str> = Vec::new();

    for stem in &targets {
        match resolve_model(&upstream_models, table[*stem]) {
            Some((src, version)) => {
                let dest = models_dir.join(format!("{stem}.json"));
                std::fs::copy(&src, &dest)
                    .map_err(|e| format!("copy {} -> {}: {e}", src.display(), dest.display()))?;
                println!("  vendored {stem:30} ({version})");
                vendored.push(((*stem).to_string(), version));
            }
            None => {
                eprintln!(
                    "  MISS     {stem:30} (no upstream dir among: {})",
                    table[*stem].join(", ")
                );
                missed.push(stem);
            }
        }
    }

    write_provenance(&models_dir, &opts.git_ref, &sha, &commit_date, &vendored)?;
    let _ = std::fs::remove_dir_all(&tmp);

    println!(
        "\ndone: {} vendored, {} missed (source {} @ {})",
        vendored.len(),
        missed.len(),
        sha.get(..12).unwrap_or(&sha),
        commit_date
    );
    if !missed.is_empty() {
        eprintln!(
            "missed: {} -- add the correct upstream dir to the SERVICES table in xtask",
            missed.join(", ")
        );
    }
    Ok(())
}

/// Partial + sparse clone of the upstream model repo, pinned to `git_ref`.
/// Only blobs under the requested `paths` are fetched.
fn sparse_checkout(dir: &Path, git_ref: &str, paths: &[String]) -> Result<(), String> {
    git(dir, &["init", "-q"])?;
    git(dir, &["remote", "add", "origin", UPSTREAM])?;
    git(dir, &["sparse-checkout", "init", "--cone"])?;
    let mut set_args = vec!["sparse-checkout", "set"];
    set_args.extend(paths.iter().map(String::as_str));
    git(dir, &set_args)?;
    git(
        dir,
        &[
            "fetch",
            "--depth",
            "1",
            "--filter=blob:none",
            "origin",
            git_ref,
        ],
    )?;
    git(dir, &["checkout", "-q", "FETCH_HEAD"])?;
    Ok(())
}

/// Find the newest `.json` model for a service among its candidate dirs.
/// Upstream layout: `models/<dir>/service/<version>/<file>.json`. Version dirs
/// are ISO dates, so the lexicographically greatest one is the latest.
fn resolve_model(upstream_models: &Path, candidates: &[&str]) -> Option<(PathBuf, String)> {
    for dir in candidates {
        let service_dir = upstream_models.join(dir).join("service");
        let Ok(versions) = std::fs::read_dir(&service_dir) else {
            continue;
        };
        let mut version_dirs: Vec<PathBuf> = versions
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        version_dirs.sort();
        let Some(latest) = version_dirs.pop() else {
            continue;
        };
        let json = std::fs::read_dir(&latest)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))?;
        let version = latest.file_name()?.to_string_lossy().into_owned();
        return Some((json, version));
    }
    None
}

fn write_provenance(
    models_dir: &Path,
    git_ref: &str,
    sha: &str,
    commit_date: &str,
    vendored: &[(String, String)],
) -> Result<(), String> {
    let mut body = String::new();
    body.push_str("# Vendored Smithy models\n\n");
    body.push_str(&format!("Source: {UPSTREAM}\n"));
    body.push_str(&format!("Ref:    {git_ref}\n"));
    body.push_str(&format!("Commit: {sha}\n"));
    body.push_str(&format!("Date:   {commit_date}\n\n"));
    body.push_str("Regenerate with `cargo xtask update-models --all`.\n\n");
    body.push_str("| model | upstream version |\n|---|---|\n");
    let mut rows = vendored.to_vec();
    rows.sort();
    for (stem, version) in rows {
        body.push_str(&format!("| {stem} | {version} |\n"));
    }
    let path = models_dir.join("PROVENANCE.md");
    std::fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("running git {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!("git {} failed", args.join(" ")));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("git output not utf-8: {e}"))
}

/// Workspace root = the directory above this crate's manifest dir.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask manifest has a parent")
        .to_path_buf()
}
