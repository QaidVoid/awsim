//! `awsim seed --file seed.toml` — reads a TOML scenario config and
//! POSTs to /_awsim/seed/<service> in sequence so a CI run can
//! reproduce a fixture.
//!
//! Example seed.toml:
//!
//! ```toml
//! endpoint = "http://localhost:4566"   # optional, defaults below
//!
//! # Each section is optional — omit a service to skip it.
//!
//! [[cognito_users]]
//! pool_id = "us-east-1_abcdef"
//! count   = 1000
//!
//! [dynamodb]
//! tables          = 5
//! items_per_table = 1000
//!
//! [s3]
//! buckets             = 5
//! objects_per_bucket  = 100
//! body_bytes          = 256
//!
//! [secrets]
//! count = 20
//!
//! [sqs]
//! queues             = 5
//! messages_per_queue = 50
//! ```

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Default)]
pub struct SeedFile {
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default, rename = "cognito_users")]
    pub cognito_users: Vec<CognitoUsers>,
    #[serde(default)]
    pub dynamodb: Option<Dynamodb>,
    #[serde(default)]
    pub s3: Option<S3>,
    #[serde(default)]
    pub secrets: Option<Secrets>,
    #[serde(default)]
    pub sqs: Option<Sqs>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CognitoUsers {
    pub pool_id: String,
    pub count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Dynamodb {
    pub tables: u64,
    #[serde(default)]
    pub items_per_table: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct S3 {
    pub buckets: u64,
    #[serde(default)]
    pub objects_per_bucket: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Secrets {
    pub count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Sqs {
    pub queues: u64,
    #[serde(default)]
    pub messages_per_queue: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

const DEFAULT_ENDPOINT: &str = "http://localhost:4566";

pub async fn run(file: &Path, cli_endpoint: Option<&str>) -> Result<()> {
    let raw = fs::read_to_string(file)
        .with_context(|| format!("reading seed file {}", file.display()))?;
    let cfg: SeedFile =
        toml::from_str(&raw).with_context(|| format!("parsing seed file {}", file.display()))?;
    let endpoint = cli_endpoint
        .map(str::to_string)
        .or(cfg.endpoint)
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());
    let client = reqwest::Client::new();

    for cog in &cfg.cognito_users {
        post(&client, &endpoint, "cognito-users", cog, "Cognito users").await?;
    }
    if let Some(d) = &cfg.dynamodb {
        post(&client, &endpoint, "dynamodb", d, "DynamoDB").await?;
    }
    if let Some(s) = &cfg.s3 {
        post(&client, &endpoint, "s3", s, "S3").await?;
    }
    if let Some(s) = &cfg.secrets {
        post(&client, &endpoint, "secrets", s, "Secrets Manager").await?;
    }
    if let Some(q) = &cfg.sqs {
        post(&client, &endpoint, "sqs", q, "SQS").await?;
    }
    println!("✓ Seed complete.");
    Ok(())
}

async fn post<B: Serialize>(
    client: &reqwest::Client,
    endpoint: &str,
    path: &str,
    body: &B,
    label: &str,
) -> Result<()> {
    let url = format!("{endpoint}/_awsim/seed/{path}");
    let res = client
        .post(&url)
        .json(body)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = res.status();
    let payload: Value = res.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        anyhow::bail!("{label} seed failed ({status}): {payload}");
    }
    println!("✓ {label}: {payload}");
    Ok(())
}
