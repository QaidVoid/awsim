use dashmap::DashMap;
use std::collections::HashMap;

/// Per-account/region Bedrock state.
#[derive(Debug, Default)]
pub struct BedrockState {
    /// guardrail_id → Guardrail
    pub guardrails: DashMap<String, Guardrail>,
    /// job_id → CustomizationJob
    pub customization_jobs: DashMap<String, CustomizationJob>,
    /// resource_arn → tags
    pub tags: DashMap<String, HashMap<String, String>>,
    /// Stored logging configuration (one per state)
    pub logging_config: DashMap<String, LoggingConfig>,
}

#[derive(Debug, Clone)]
pub struct Guardrail {
    pub guardrail_id: String,
    pub name: String,
    pub arn: String,
    pub blocked_input_messaging: String,
    pub blocked_outputs_messaging: String,
    pub status: String,
    pub created_at: String,
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct CustomizationJob {
    pub job_arn: String,
    pub base_model_identifier: String,
    pub custom_model_name: String,
    pub status: String,
    pub creation_time: String,
}

/// Model invocation logging configuration.
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    pub cloud_watch_config: Option<serde_json::Value>,
    pub s3_config: Option<serde_json::Value>,
    pub embedding_data_delivery_enabled: bool,
    pub image_data_delivery_enabled: bool,
    pub text_data_delivery_enabled: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            cloud_watch_config: None,
            s3_config: None,
            embedding_data_delivery_enabled: false,
            image_data_delivery_enabled: false,
            text_data_delivery_enabled: false,
        }
    }
}

pub fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    let year = 1970 + days / 365;
    let day_of_year = days % 365;
    let month = day_of_year / 30 + 1;
    let day = day_of_year % 30 + 1;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, h, m, s
    )
}
