use dashmap::DashMap;
use std::collections::HashMap;

/// Top-level API Gateway state — shared across all accounts/regions
/// (API Gateway v2 uses a single global namespace per region in real AWS,
/// but we store it in the AccountRegionStore pattern for consistency).
#[derive(Debug, Default)]
pub struct ApiGatewayState {
    pub apis: DashMap<String, HttpApi>,
}

#[derive(Debug, Clone)]
pub struct HttpApi {
    pub api_id: String,
    pub name: String,
    pub protocol_type: String,
    /// e.g., "http://localhost:4566/restapis/{api_id}"
    pub api_endpoint: String,
    pub routes: HashMap<String, ApiRoute>,
    pub integrations: HashMap<String, Integration>,
    pub stages: HashMap<String, Stage>,
    pub deployments: HashMap<String, Deployment>,
    pub created_date: String,
    pub description: String,
    pub cors_configuration: Option<CorsConfiguration>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ApiRoute {
    pub route_id: String,
    /// e.g., "GET /items", "POST /items/{id}", "$default"
    pub route_key: String,
    /// e.g., "integrations/{integration_id}"
    pub target: Option<String>,
    pub route_response_selection_expression: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Integration {
    pub integration_id: String,
    pub integration_type: String,  // "AWS_PROXY", "HTTP_PROXY", etc.
    pub integration_uri: String,   // Lambda function ARN
    pub payload_format_version: String, // "1.0" or "2.0"
    pub integration_method: Option<String>,
    pub description: Option<String>,
    pub timeout_in_millis: u32,
}

#[derive(Debug, Clone)]
pub struct Stage {
    pub stage_name: String,
    pub auto_deploy: bool,
    pub description: String,
    pub deployment_id: Option<String>,
    pub created_date: String,
    pub last_updated_date: String,
    pub default_route_settings: RouteSettings,
}

#[derive(Debug, Clone, Default)]
pub struct RouteSettings {
    pub throttling_burst_limit: Option<u32>,
    pub throttling_rate_limit: Option<f64>,
    pub logging_level: Option<String>,
    pub data_trace_enabled: bool,
    pub detailed_metrics_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct Deployment {
    pub deployment_id: String,
    pub deployment_status: String,
    pub created_date: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CorsConfiguration {
    pub allow_origins: Vec<String>,
    pub allow_methods: Vec<String>,
    pub allow_headers: Vec<String>,
    pub expose_headers: Vec<String>,
    pub max_age: Option<u32>,
    pub allow_credentials: bool,
}
