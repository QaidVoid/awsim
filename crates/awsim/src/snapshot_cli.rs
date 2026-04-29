//! `awsim snapshot` — list / save / load / delete named state
//! snapshots on a running awsim instance.

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::SnapshotCommand;

pub async fn run(cmd: SnapshotCommand) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("build HTTP client")?;
    match cmd {
        SnapshotCommand::List { endpoint, json } => list(&client, &endpoint, json).await,
        SnapshotCommand::Save { endpoint, name } => save(&client, &endpoint, &name).await,
        SnapshotCommand::Load { endpoint, name } => load(&client, &endpoint, &name).await,
        SnapshotCommand::Delete { endpoint, name } => delete(&client, &endpoint, &name).await,
    }
}

async fn list(client: &reqwest::Client, endpoint: &str, as_json: bool) -> Result<()> {
    let url = format!("{}/_awsim/snapshots", trim(endpoint));
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("connect to {endpoint}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("HTTP {status}: {text}");
    }
    let body: Value = resp.json().await.context("parse response")?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }
    let snaps = body["snapshots"].as_array().cloned().unwrap_or_default();
    if snaps.is_empty() {
        println!("No named snapshots.");
        return Ok(());
    }
    for s in snaps {
        let name = s["name"].as_str().unwrap_or("?");
        let ts = s["created_ts"].as_u64().unwrap_or(0);
        let services = s["services"].as_array().map(|a| a.len()).unwrap_or(0);
        let billing = if s["has_billing"].as_bool().unwrap_or(false) {
            "+billing"
        } else {
            ""
        };
        let chaos = if s["has_chaos"].as_bool().unwrap_or(false) {
            "+chaos"
        } else {
            ""
        };
        println!(
            "  {name:24}  {ts}  {services} service(s) {billing} {chaos}",
            name = name,
            ts = format_ts(ts),
            services = services,
        );
    }
    Ok(())
}

async fn save(client: &reqwest::Client, endpoint: &str, name: &str) -> Result<()> {
    let url = format!("{}/_awsim/snapshots/{name}", trim(endpoint));
    let resp = client
        .post(&url)
        .send()
        .await
        .with_context(|| format!("connect to {endpoint}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("HTTP {status}: {text}");
    }
    let v: Value = resp.json().await.context("parse response")?;
    let services = v["services"].as_array().map(|a| a.len()).unwrap_or(0);
    println!("saved snapshot `{name}` ({services} service(s))");
    Ok(())
}

async fn load(client: &reqwest::Client, endpoint: &str, name: &str) -> Result<()> {
    let url = format!("{}/_awsim/snapshots/{name}/load", trim(endpoint));
    let resp = client
        .post(&url)
        .send()
        .await
        .with_context(|| format!("connect to {endpoint}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("HTTP {status}: {text}");
    }
    let v: Value = resp.json().await.context("parse response")?;
    let restored = v["restored"].as_array().map(|a| a.len()).unwrap_or(0);
    let failed = v["failed"].as_array().cloned().unwrap_or_default();
    println!("loaded snapshot `{name}` ({restored} service(s) restored)");
    if !failed.is_empty() {
        eprintln!("  ! {} service(s) failed:", failed.len());
        for f in failed {
            let svc = f["service"].as_str().unwrap_or("?");
            let err = f["error"].as_str().unwrap_or("?");
            eprintln!("    - {svc}: {err}");
        }
    }
    Ok(())
}

async fn delete(client: &reqwest::Client, endpoint: &str, name: &str) -> Result<()> {
    let url = format!("{}/_awsim/snapshots/{name}", trim(endpoint));
    let resp = client
        .delete(&url)
        .send()
        .await
        .with_context(|| format!("connect to {endpoint}"))?;
    if !resp.status().is_success() {
        bail!("HTTP {}: snapshot not found", resp.status());
    }
    println!("deleted snapshot `{name}`");
    Ok(())
}

fn trim(endpoint: &str) -> &str {
    endpoint.trim_end_matches('/')
}

/// Format unix-seconds as a short relative+absolute label, e.g.
/// "3m ago (2026-04-29 10:30)". Skips the relative prefix for very
/// old timestamps.
fn format_ts(ts: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let diff = now.saturating_sub(ts);
    if diff < 60 {
        format!("{diff}s ago")
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86_400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86_400)
    }
}
