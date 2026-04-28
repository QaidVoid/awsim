use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// A single dimension attached to a metric datum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dimension {
    pub name: String,
    pub value: String,
}

/// A single stored metric data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDatum {
    pub metric_name: String,
    pub namespace: String,
    pub value: f64,
    pub unit: String,
    pub timestamp: String,
    pub dimensions: Vec<Dimension>,
}

/// A CloudWatch metric alarm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricAlarm {
    pub alarm_name: String,
    pub metric_name: String,
    pub namespace: String,
    pub statistic: String,
    pub period: u64,
    pub evaluation_periods: u64,
    pub threshold: f64,
    pub comparison_operator: String,
    pub state_value: String,
    pub state_reason: String,
    pub actions_enabled: bool,
    pub alarm_actions: Vec<String>,
    pub created_at: String,
    /// Updated each time the evaluator transitions the alarm.
    #[serde(default)]
    pub state_updated_at: Option<String>,
    /// Dimension filter — only metric data points carrying these dimensions
    /// participate in evaluation. When empty the alarm matches any datum
    /// for the (Namespace, MetricName) pair.
    #[serde(default)]
    pub dimensions: Vec<Dimension>,
}

/// A stored CloudWatch dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dashboard {
    pub name: String,
    pub body: String,
}

/// Per-account/region CloudWatch Metrics state.
#[derive(Debug, Default)]
pub struct CloudWatchState {
    /// namespace → list of data points
    pub metrics: DashMap<String, Vec<MetricDatum>>,
    /// alarm name → alarm
    pub alarms: DashMap<String, MetricAlarm>,
    /// dashboard name → dashboard
    pub dashboards: DashMap<String, Dashboard>,
}
