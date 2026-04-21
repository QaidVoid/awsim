use std::collections::HashMap;

use dashmap::DashMap;

/// Per-account/region AppSync state.
#[derive(Debug, Default)]
pub struct AppSyncState {
    /// api_id → GraphqlApi
    pub apis: DashMap<String, GraphqlApi>,
}

#[derive(Debug, Clone)]
pub struct GraphqlApi {
    pub api_id: String,
    pub name: String,
    pub arn: String,
    pub uris: HashMap<String, String>,
    pub authentication_type: String,
    pub schema: Option<String>,
    pub schema_status: String,
    pub api_keys: Vec<ApiKey>,
    pub data_sources: Vec<DataSource>,
    pub resolvers: Vec<Resolver>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ApiKey {
    pub id: String,
    pub description: Option<String>,
    pub expires: i64,
}

#[derive(Debug, Clone)]
pub struct DataSource {
    pub name: String,
    pub data_source_type: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Resolver {
    pub type_name: String,
    pub field_name: String,
    pub data_source_name: String,
}

pub fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Simple ISO 8601 format
    let dt = secs;
    let s = dt % 60;
    let m = (dt / 60) % 60;
    let h = (dt / 3600) % 24;
    let days = dt / 86400;
    // Approximate date calculation from epoch
    let year = 1970 + days / 365;
    let day_of_year = days % 365;
    let month = day_of_year / 30 + 1;
    let day = day_of_year % 30 + 1;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, h, m, s
    )
}
