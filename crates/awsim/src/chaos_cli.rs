//! `awsim chaos` — manage rules on a running awsim instance.

use anyhow::{Context, Result, bail};
use awsim_chaos::{
    ChaosEffect, ChaosRule, ErrorEffect, LatencyEffect, OperationMatch, ServiceMatch,
};
use serde_json::{Value, json};

use crate::{ChaosCommand, ChaosPresetCommand};

pub async fn run(cmd: ChaosCommand) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("build HTTP client")?;
    match cmd {
        ChaosCommand::List { endpoint, json } => list(&client, &endpoint, json).await,
        ChaosCommand::Add {
            endpoint,
            service,
            operation,
            probability,
            error,
            latency,
            label,
        } => {
            let effect = parse_effect(error.as_deref(), latency.as_deref())?;
            add(
                &client,
                &endpoint,
                &service,
                &operation,
                probability,
                effect,
                label.as_deref(),
            )
            .await
        }
        ChaosCommand::Remove { endpoint, id } => remove(&client, &endpoint, &id).await,
        ChaosCommand::Clear { endpoint } => clear(&client, &endpoint).await,
        ChaosCommand::Stats { endpoint } => stats(&client, &endpoint).await,
        ChaosCommand::Preset { command } => match command {
            ChaosPresetCommand::List { endpoint, json } => {
                preset_list(&client, &endpoint, json).await
            }
            ChaosPresetCommand::Apply { endpoint, name } => {
                preset_apply(&client, &endpoint, &name).await
            }
        },
    }
}

fn parse_effect(error: Option<&str>, latency: Option<&str>) -> Result<ChaosEffect> {
    match (error, latency) {
        (None, None) => bail!("specify at least one of --error or --latency"),
        (Some(e), None) => Ok(ChaosEffect::Error(parse_error(e)?)),
        (None, Some(l)) => Ok(ChaosEffect::Latency(parse_latency(l)?)),
        (Some(e), Some(l)) => Ok(ChaosEffect::Both {
            latency: parse_latency(l)?,
            error: parse_error(e)?,
        }),
    }
}

/// `STATUS,CODE[,MESSAGE]` — e.g. `503,SlowDown,please retry`.
fn parse_error(spec: &str) -> Result<ErrorEffect> {
    let mut parts = spec.splitn(3, ',');
    let status_str = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("error spec missing status"))?;
    let status: u16 = status_str
        .trim()
        .parse()
        .with_context(|| format!("invalid status `{status_str}`"))?;
    let code = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("error spec missing code"))?
        .trim()
        .to_string();
    let message = parts
        .next()
        .map(|m| m.trim().to_string())
        .unwrap_or_else(|| format!("synthetic {code}"));
    Ok(ErrorEffect {
        status,
        code,
        message,
        retry_after_secs: None,
    })
}

/// `MIN-MAX` or `MS` — e.g. `100-500` for a range, `200` for fixed.
fn parse_latency(spec: &str) -> Result<LatencyEffect> {
    if let Some((min, max)) = spec.split_once('-') {
        let min_ms: u64 = min
            .trim()
            .parse()
            .with_context(|| format!("invalid min `{min}`"))?;
        let max_ms: u64 = max
            .trim()
            .parse()
            .with_context(|| format!("invalid max `{max}`"))?;
        if max_ms < min_ms {
            bail!("latency max ({max_ms}) must be >= min ({min_ms})");
        }
        Ok(LatencyEffect { min_ms, max_ms })
    } else {
        let ms: u64 = spec
            .trim()
            .parse()
            .with_context(|| format!("invalid latency `{spec}`"))?;
        Ok(LatencyEffect {
            min_ms: ms,
            max_ms: ms,
        })
    }
}

fn service_match(s: &str) -> ServiceMatch {
    if s == "*" {
        ServiceMatch::Any
    } else {
        ServiceMatch::Exact(s.to_string())
    }
}

fn operation_match(s: &str) -> OperationMatch {
    if s == "*" {
        OperationMatch::Any
    } else {
        OperationMatch::Exact(s.to_string())
    }
}

async fn list(client: &reqwest::Client, endpoint: &str, as_json: bool) -> Result<()> {
    let url = format!("{}/_awsim/chaos/rules", trim(endpoint));
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("connect to {endpoint}"))?
        .error_for_status()
        .context("list rules")?;
    let body: Value = resp.json().await.context("parse response")?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }
    let rules: Vec<ChaosRule> = serde_json::from_value(body["rules"].clone()).unwrap_or_default();
    let total = body["total_injections"].as_u64().unwrap_or(0);
    if rules.is_empty() {
        println!("No chaos rules. Total injections: {total}");
        return Ok(());
    }
    println!("Total injections: {total}");
    println!();
    for r in &rules {
        let svc = match &r.service {
            ServiceMatch::Any => "*".to_string(),
            ServiceMatch::Exact(s) => s.clone(),
        };
        let op = match &r.operation {
            OperationMatch::Any => "*".to_string(),
            OperationMatch::Exact(s) => s.clone(),
        };
        let effect_str = describe_effect(&r.effect);
        let enabled = if r.enabled { " " } else { "✗" };
        println!(
            "  {enabled} {id}  {svc}/{op}  p={p:.2}  {effect_str}  fired={count}",
            id = &r.id[..r.id.len().min(8)],
            p = r.probability,
            count = r.injection_count,
        );
        if let Some(label) = &r.label {
            println!("       └ {label}");
        }
    }
    Ok(())
}

fn describe_effect(eff: &ChaosEffect) -> String {
    match eff {
        ChaosEffect::Error(e) => format!("[{}] {}", e.status, e.code),
        ChaosEffect::Latency(l) if l.min_ms == l.max_ms => format!("+{}ms", l.min_ms),
        ChaosEffect::Latency(l) => format!("+{}-{}ms", l.min_ms, l.max_ms),
        ChaosEffect::Both { latency, error } => {
            let lat = if latency.min_ms == latency.max_ms {
                format!("+{}ms", latency.min_ms)
            } else {
                format!("+{}-{}ms", latency.min_ms, latency.max_ms)
            };
            format!("{lat} then [{}] {}", error.status, error.code)
        }
    }
}

async fn add(
    client: &reqwest::Client,
    endpoint: &str,
    service: &str,
    operation: &str,
    probability: f64,
    effect: ChaosEffect,
    label: Option<&str>,
) -> Result<()> {
    let body = json!({
        "id": "",
        "service": service_match(service),
        "operation": operation_match(operation),
        "probability": probability,
        "effect": effect,
        "enabled": true,
        "label": label,
    });
    let url = format!("{}/_awsim/chaos/rules", trim(endpoint));
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("connect to {endpoint}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("HTTP {status}: {text}");
    }
    let v: Value = resp.json().await.context("parse response")?;
    println!("added rule {}", v["id"].as_str().unwrap_or("?"));
    Ok(())
}

async fn remove(client: &reqwest::Client, endpoint: &str, id: &str) -> Result<()> {
    let url = format!("{}/_awsim/chaos/rules/{id}", trim(endpoint));
    let resp = client
        .delete(&url)
        .send()
        .await
        .with_context(|| format!("connect to {endpoint}"))?;
    if !resp.status().is_success() {
        bail!("HTTP {}: rule not found", resp.status());
    }
    println!("removed rule {id}");
    Ok(())
}

async fn clear(client: &reqwest::Client, endpoint: &str) -> Result<()> {
    let url = format!("{}/_awsim/chaos/clear", trim(endpoint));
    let resp = client
        .post(&url)
        .send()
        .await
        .with_context(|| format!("connect to {endpoint}"))?;
    if !resp.status().is_success() {
        bail!("HTTP {}", resp.status());
    }
    println!("cleared all chaos rules");
    Ok(())
}

async fn stats(client: &reqwest::Client, endpoint: &str) -> Result<()> {
    let url = format!("{}/_awsim/chaos/stats", trim(endpoint));
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("connect to {endpoint}"))?
        .error_for_status()
        .context("fetch stats")?;
    let body: Value = resp.json().await.context("parse response")?;
    let total = body["total_injections"].as_u64().unwrap_or(0);
    println!("Total injections: {total}");
    if let Some(arr) = body["recent"].as_array()
        && !arr.is_empty()
    {
        println!("\nRecent (newest last):");
        for entry in arr.iter().rev().take(20).collect::<Vec<_>>().iter().rev() {
            let svc = entry["service"].as_str().unwrap_or("?");
            let op = entry["operation"].as_str().unwrap_or("?");
            let ts = entry["ts"].as_u64().unwrap_or(0);
            println!("  {ts}  {svc}/{op}");
        }
    }
    Ok(())
}

async fn preset_list(client: &reqwest::Client, endpoint: &str, as_json: bool) -> Result<()> {
    let url = format!("{}/_awsim/chaos/presets", trim(endpoint));
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("connect to {endpoint}"))?
        .error_for_status()
        .context("list presets")?;
    let body: Value = resp.json().await.context("parse response")?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }
    let entries = body["presets"].as_array().cloned().unwrap_or_default();
    if entries.is_empty() {
        println!("No presets registered.");
        return Ok(());
    }
    for p in entries {
        let name = p["name"].as_str().unwrap_or("?");
        let desc = p["description"].as_str().unwrap_or("");
        println!("  {name:20}  {desc}");
    }
    Ok(())
}

async fn preset_apply(client: &reqwest::Client, endpoint: &str, name: &str) -> Result<()> {
    let url = format!("{}/_awsim/chaos/presets/{name}", trim(endpoint));
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
    let ids = v["rule_ids"].as_array().cloned().unwrap_or_default();
    println!("applied preset `{name}` ({} rule(s))", ids.len());
    for id in ids {
        if let Some(s) = id.as_str() {
            println!("  + {s}");
        }
    }
    Ok(())
}

fn trim(endpoint: &str) -> &str {
    endpoint.trim_end_matches('/')
}
