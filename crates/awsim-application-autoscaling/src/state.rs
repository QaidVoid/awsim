use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default)]
pub struct AppAutoScalingState {
    pub targets: DashMap<String, ScalableTarget>,
    pub policies: DashMap<String, ScalingPolicy>,
    pub scheduled_actions: DashMap<String, ScheduledAction>,
}

/// Composite key shared by every resource type:
/// `{ServiceNamespace}|{ResourceId}|{ScalableDimension}`.
pub fn target_key(service_namespace: &str, resource_id: &str, dimension: &str) -> String {
    format!("{service_namespace}|{resource_id}|{dimension}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalableTarget {
    pub service_namespace: String,
    pub resource_id: String,
    pub scalable_dimension: String,
    pub min_capacity: i32,
    pub max_capacity: i32,
    pub role_arn: String,
    pub creation_time: f64,
    pub suspended_state: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingPolicy {
    pub policy_name: String,
    pub policy_arn: String,
    pub service_namespace: String,
    pub resource_id: String,
    pub scalable_dimension: String,
    pub policy_type: String,
    pub step_scaling_policy_configuration: Option<serde_json::Value>,
    pub target_tracking_scaling_policy_configuration: Option<serde_json::Value>,
    pub creation_time: f64,
    pub alarms: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledAction {
    pub scheduled_action_name: String,
    pub scheduled_action_arn: String,
    pub service_namespace: String,
    pub schedule: String,
    pub timezone: Option<String>,
    pub resource_id: String,
    pub scalable_dimension: String,
    pub start_time: Option<f64>,
    pub end_time: Option<f64>,
    pub scalable_target_action: serde_json::Value,
    pub creation_time: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppAutoScalingSnapshot {
    pub targets: Vec<ScalableTarget>,
    pub policies: Vec<ScalingPolicy>,
    pub scheduled_actions: Vec<ScheduledAction>,
}

impl AppAutoScalingState {
    pub fn to_snapshot(&self) -> AppAutoScalingSnapshot {
        AppAutoScalingSnapshot {
            targets: self.targets.iter().map(|e| e.value().clone()).collect(),
            policies: self.policies.iter().map(|e| e.value().clone()).collect(),
            scheduled_actions: self
                .scheduled_actions
                .iter()
                .map(|e| e.value().clone())
                .collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snap: AppAutoScalingSnapshot) {
        self.targets.clear();
        self.policies.clear();
        self.scheduled_actions.clear();
        for t in snap.targets {
            self.targets.insert(
                target_key(&t.service_namespace, &t.resource_id, &t.scalable_dimension),
                t,
            );
        }
        for p in snap.policies {
            self.policies.insert(
                policy_key(
                    &p.service_namespace,
                    &p.resource_id,
                    &p.scalable_dimension,
                    &p.policy_name,
                ),
                p,
            );
        }
        for a in snap.scheduled_actions {
            self.scheduled_actions.insert(
                scheduled_key(
                    &a.service_namespace,
                    &a.resource_id,
                    &a.scalable_dimension,
                    &a.scheduled_action_name,
                ),
                a,
            );
        }
    }
}

pub fn policy_key(ns: &str, rid: &str, dim: &str, name: &str) -> String {
    format!("{ns}|{rid}|{dim}|{name}")
}

pub fn scheduled_key(ns: &str, rid: &str, dim: &str, name: &str) -> String {
    format!("{ns}|{rid}|{dim}|{name}")
}
