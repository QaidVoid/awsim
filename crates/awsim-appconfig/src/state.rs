use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct AppConfigState {
    pub applications: DashMap<String, Application>,
    /// (app_id, env_id) keyed.
    pub environments: DashMap<String, Environment>,
    /// (app_id, profile_id) keyed.
    pub profiles: DashMap<String, ConfigProfile>,
    /// (app_id, profile_id, version_number) keyed.
    pub hosted_versions: DashMap<String, HostedConfigVersion>,
    /// (app_id, env_id, deployment_number) keyed.
    pub deployments: DashMap<String, Deployment>,
    pub deployment_strategies: DashMap<String, DeploymentStrategy>,
    /// session token → (app_id, env_id, profile_id).
    pub sessions: DashMap<String, ConfigurationSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub id: String,
    pub application_id: String,
    pub name: String,
    pub description: Option<String>,
    pub state: String,
    pub monitors: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigProfile {
    pub id: String,
    pub application_id: String,
    pub name: String,
    pub location_uri: String,
    pub retrieval_role_arn: Option<String>,
    pub r#type: String,
    pub validators: Vec<serde_json::Value>,
    pub description: Option<String>,
    pub latest_version_number: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedConfigVersion {
    pub application_id: String,
    pub configuration_profile_id: String,
    pub version_number: u32,
    pub description: Option<String>,
    pub content: Vec<u8>,
    pub content_type: String,
    pub version_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deployment {
    pub application_id: String,
    pub environment_id: String,
    pub deployment_number: u32,
    pub configuration_profile_id: String,
    pub deployment_strategy_id: String,
    pub configuration_version: String,
    pub state: String,
    pub percentage_complete: f64,
    pub event_log: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentStrategy {
    pub id: String,
    pub name: String,
    pub deployment_duration_in_minutes: u32,
    pub growth_factor: f64,
    pub final_bake_time_in_minutes: u32,
    pub growth_type: String,
    pub replicate_to: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurationSession {
    pub token: String,
    pub application_identifier: String,
    pub environment_identifier: String,
    pub configuration_profile_identifier: String,
    /// Last version the client received — used to gate "no change" responses.
    pub last_version_label: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppConfigSnapshot {
    pub applications: Vec<Application>,
    pub environments: Vec<Environment>,
    pub profiles: Vec<ConfigProfile>,
    pub hosted_versions: Vec<HostedConfigVersion>,
    pub deployments: Vec<Deployment>,
    pub deployment_strategies: Vec<DeploymentStrategy>,
    pub sessions: HashMap<String, ConfigurationSession>,
}

pub fn env_key(app_id: &str, env_id: &str) -> String {
    format!("{app_id}:{env_id}")
}

pub fn profile_key(app_id: &str, profile_id: &str) -> String {
    format!("{app_id}:{profile_id}")
}

pub fn hosted_key(app_id: &str, profile_id: &str, version: u32) -> String {
    format!("{app_id}:{profile_id}:{version}")
}

pub fn deployment_key(app_id: &str, env_id: &str, num: u32) -> String {
    format!("{app_id}:{env_id}:{num}")
}

impl AppConfigState {
    pub fn to_snapshot(&self) -> AppConfigSnapshot {
        AppConfigSnapshot {
            applications: self
                .applications
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            environments: self
                .environments
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            profiles: self.profiles.iter().map(|e| e.value().clone()).collect(),
            hosted_versions: self
                .hosted_versions
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            deployments: self.deployments.iter().map(|e| e.value().clone()).collect(),
            deployment_strategies: self
                .deployment_strategies
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            sessions: self
                .sessions
                .iter()
                .map(|e| (e.key().clone(), e.value().clone()))
                .collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snap: AppConfigSnapshot) {
        self.applications.clear();
        self.environments.clear();
        self.profiles.clear();
        self.hosted_versions.clear();
        self.deployments.clear();
        self.deployment_strategies.clear();
        self.sessions.clear();
        for a in snap.applications {
            self.applications.insert(a.id.clone(), a);
        }
        for e in snap.environments {
            self.environments
                .insert(env_key(&e.application_id, &e.id), e);
        }
        for p in snap.profiles {
            self.profiles
                .insert(profile_key(&p.application_id, &p.id), p);
        }
        for h in snap.hosted_versions {
            self.hosted_versions.insert(
                hosted_key(
                    &h.application_id,
                    &h.configuration_profile_id,
                    h.version_number,
                ),
                h,
            );
        }
        for d in snap.deployments {
            self.deployments.insert(
                deployment_key(&d.application_id, &d.environment_id, d.deployment_number),
                d,
            );
        }
        for s in snap.deployment_strategies {
            self.deployment_strategies.insert(s.id.clone(), s);
        }
        for (k, v) in snap.sessions {
            self.sessions.insert(k, v);
        }
    }
}
