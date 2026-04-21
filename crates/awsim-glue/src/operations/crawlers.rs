use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use tracing::info;

use crate::state::{Crawler, GlueState};

fn now_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

// ---------------------------------------------------------------------------
// CreateCrawler
// ---------------------------------------------------------------------------

pub fn create_crawler(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;
    let role = input["Role"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Role is required"))?;

    if state.crawlers.contains_key(name) {
        return Err(AwsError::conflict(
            "AlreadyExistsException",
            format!("Crawler already exists: {name}"),
        ));
    }

    let targets = input.get("Targets").cloned();
    let database_name = input["DatabaseName"].as_str().map(|s| s.to_string());

    let crawler = Crawler {
        name: name.to_string(),
        role: role.to_string(),
        database_name,
        targets,
        state: "READY".to_string(),
        created_at: now_str(),
    };

    info!(name = %name, "Created Glue crawler");
    state.crawlers.insert(name.to_string(), crawler);

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// GetCrawler
// ---------------------------------------------------------------------------

pub fn get_crawler(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    let crawler = state.crawlers.get(name).ok_or_else(|| {
        AwsError::not_found("EntityNotFoundException", format!("Crawler not found: {name}"))
    })?;

    Ok(json!({ "Crawler": crawler_to_value(&crawler) }))
}

// ---------------------------------------------------------------------------
// GetCrawlers
// ---------------------------------------------------------------------------

pub fn get_crawlers(
    state: &GlueState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let list: Vec<Value> = state
        .crawlers
        .iter()
        .map(|e| crawler_to_value(e.value()))
        .collect();

    Ok(json!({ "Crawlers": list }))
}

// ---------------------------------------------------------------------------
// DeleteCrawler
// ---------------------------------------------------------------------------

pub fn delete_crawler(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    state.crawlers.remove(name).ok_or_else(|| {
        AwsError::not_found("EntityNotFoundException", format!("Crawler not found: {name}"))
    })?;

    info!(name = %name, "Deleted Glue crawler");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// StartCrawler
// ---------------------------------------------------------------------------

pub fn start_crawler(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    {
        let mut crawler = state.crawlers.get_mut(name).ok_or_else(|| {
            AwsError::not_found("EntityNotFoundException", format!("Crawler not found: {name}"))
        })?;

        if crawler.state == "RUNNING" {
            return Err(AwsError::conflict(
                "CrawlerRunningException",
                format!("Crawler is already running: {name}"),
            ));
        }

        // Stub: transition READY → RUNNING → READY (immediately READY for metadata stub).
        crawler.state = "READY".to_string();
    }

    info!(name = %name, "Started Glue crawler (stub: immediately READY)");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// StopCrawler
// ---------------------------------------------------------------------------

pub fn stop_crawler(
    state: &GlueState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidInputException", "Name is required"))?;

    {
        let mut crawler = state.crawlers.get_mut(name).ok_or_else(|| {
            AwsError::not_found("EntityNotFoundException", format!("Crawler not found: {name}"))
        })?;

        crawler.state = "READY".to_string();
    }

    info!(name = %name, "Stopped Glue crawler");
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn crawler_to_value(c: &Crawler) -> Value {
    json!({
        "Name": c.name,
        "Role": c.role,
        "DatabaseName": c.database_name,
        "Targets": c.targets,
        "State": c.state,
        "CreationTime": c.created_at,
    })
}
