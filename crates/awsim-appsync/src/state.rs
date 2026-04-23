use std::collections::HashMap;

use dashmap::DashMap;

/// Per-account/region AppSync state.
#[derive(Debug, Default)]
pub struct AppSyncState {
    /// api_id → GraphqlApi
    pub apis: DashMap<String, GraphqlApi>,
    /// resource_arn → tags
    pub tags: DashMap<String, HashMap<String, String>>,
    /// association_id → SourceApiAssociation
    pub source_api_associations: DashMap<String, SourceApiAssociation>,
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
    pub types: Vec<GraphqlType>,
    pub functions: Vec<AppSyncFunction>,
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
    pub request_mapping_template: Option<String>,
    pub response_mapping_template: Option<String>,
}

/// A GraphQL type (SDL or JSON).
#[derive(Debug, Clone)]
pub struct GraphqlType {
    pub name: String,
    pub definition: Option<String>,
    pub format: String, // SDL | JSON
    pub arn: String,
}

/// A merged-source-API association.
#[derive(Debug, Clone)]
pub struct SourceApiAssociation {
    pub association_id: String,
    pub association_arn: String,
    pub source_api_id: String,
    pub merged_api_id: String,
    pub description: Option<String>,
    pub status: String,
    pub last_successful_merge_date: String,
}

/// An AppSync pipeline function (not Lambda).
#[derive(Debug, Clone)]
pub struct AppSyncFunction {
    pub function_id: String,
    pub function_arn: String,
    pub name: String,
    pub description: Option<String>,
    pub data_source_name: String,
    pub request_mapping_template: Option<String>,
    pub response_mapping_template: Option<String>,
    pub function_version: String,
    pub created_at: String,
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
