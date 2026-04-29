//! `awsim bill` — fetch the running instance's billing report and
//! pretty-print it (or emit raw JSON for piping into jq / scripts).

use anyhow::{Context, Result, bail};
use awsim_billing::BillingReport;

pub async fn run(endpoint: &str, json: bool) -> Result<()> {
    let url = format!("{}/_awsim/billing", endpoint.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("build HTTP client")?;
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("connect to awsim at {endpoint}"))?;
    if !resp.status().is_success() {
        bail!("awsim returned HTTP {} from {url}", resp.status().as_u16());
    }
    let body = resp.bytes().await.context("read response body")?;
    if json {
        // Pass-through: don't reparse, just print what we got.
        let s = std::str::from_utf8(&body).context("response is not utf-8")?;
        println!("{s}");
        return Ok(());
    }
    let report: BillingReport = serde_json::from_slice(&body).context("parse billing report")?;
    print_pretty(&report);
    Ok(())
}

fn print_pretty(report: &BillingReport) {
    println!(
        "Estimated monthly:  {} (over {})",
        fmt_usd(report.projected_monthly_cost_usd),
        fmt_elapsed(report.elapsed_secs)
    );
    println!(
        "Spent so far:       {}",
        fmt_usd_precise(report.running_cost_usd)
    );
    println!();

    if report.services.is_empty() {
        println!("No metered usage yet — hit some AWS endpoints to see costs.");
        return;
    }

    // Column widths sized to the data so the output stays compact.
    let name_w = report
        .services
        .iter()
        .map(|s| s.display_name.chars().count())
        .max()
        .unwrap_or(8)
        .max(7);
    let cost_strs: Vec<String> = report
        .services
        .iter()
        .map(|s| fmt_usd_precise(s.total_cost_usd))
        .collect();
    let cost_w = cost_strs.iter().map(|s| s.len()).max().unwrap_or(8).max(7);

    for (svc, cost_str) in report.services.iter().zip(cost_strs.iter()) {
        let mut detail_parts: Vec<String> = Vec::new();
        if svc.request_count > 0 {
            detail_parts.push(format!("{} reqs", fmt_count(svc.request_count)));
        }
        if svc.storage_bytes > 0 {
            detail_parts.push(format!("{} stored", fmt_bytes(svc.storage_bytes)));
        }
        if svc.compute_gb_seconds > 0.0 {
            detail_parts.push(format!("{:.3} GB·s", svc.compute_gb_seconds));
        }
        if svc.resource_count > 0 {
            detail_parts.push(format!("{} running", svc.resource_count));
        }
        if svc.error_count > 0 {
            detail_parts.push(format!("{} errors", svc.error_count));
        }
        let detail = if detail_parts.is_empty() {
            String::new()
        } else {
            format!("  · {}", detail_parts.join(" · "))
        };
        println!(
            "  {:<name_w$}  {:>cost_w$}{detail}",
            svc.display_name,
            cost_str,
            name_w = name_w,
            cost_w = cost_w,
        );
    }
}

fn fmt_usd(n: f64) -> String {
    if n == 0.0 {
        "$0.00".to_string()
    } else if n >= 10_000.0 {
        format!("${:.1}K", n / 1_000.0)
    } else if n >= 1.0 {
        format!("${n:.2}")
    } else {
        // Sub-dollar — show enough digits to be informative.
        format!("${n:.4}")
    }
}

fn fmt_usd_precise(n: f64) -> String {
    if n == 0.0 {
        "$0.00".to_string()
    } else if n.abs() < 0.01 {
        // For tiny costs use scientific notation rather than a wall
        // of leading zeros.
        format!("${n:.3e}")
    } else {
        format!("${n:.4}")
    }
}

fn fmt_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn fmt_bytes(bytes: u64) -> String {
    let bytes_f = bytes as f64;
    if bytes_f < 1024.0 {
        return format!("{bytes} B");
    }
    let units = ["KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes_f / 1024.0;
    let mut i = 0;
    while value >= 1024.0 && i < units.len() - 1 {
        value /= 1024.0;
        i += 1;
    }
    let precision = if value < 10.0 { 2 } else { 1 };
    format!("{value:.*} {}", precision, units[i])
}

fn fmt_elapsed(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    if secs < 3600 {
        return format!("{}m {}s", secs / 60, secs % 60);
    }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    format!("{h}h {m}m")
}
