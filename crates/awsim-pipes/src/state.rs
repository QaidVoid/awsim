use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct PipesState {
    pub pipes: DashMap<String, Pipe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipe {
    pub name: String,
    pub arn: String,
    pub source: String,
    pub target: String,
    /// CREATING | RUNNING | STOPPING | STOPPED | UPDATING | DELETING | CREATE_FAILED.
    pub current_state: String,
    /// User-requested state (RUNNING | STOPPED). Pipes the user has stopped
    /// stay STOPPED until explicitly StartPipe'd again.
    pub desired_state: String,
    pub state_reason: Option<String>,
    pub role_arn: String,
    pub description: Option<String>,
    pub source_parameters: Option<serde_json::Value>,
    pub target_parameters: Option<serde_json::Value>,
    pub enrichment: Option<String>,
    pub enrichment_parameters: Option<serde_json::Value>,
    pub log_configuration: Option<serde_json::Value>,
    pub tags: HashMap<String, String>,
    pub creation_time: f64,
    pub last_modified_time: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PipesStateSnapshot {
    pub pipes: Vec<Pipe>,
}

impl PipesState {
    pub fn to_snapshot(&self) -> PipesStateSnapshot {
        PipesStateSnapshot {
            pipes: self.pipes.iter().map(|e| e.value().clone()).collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snapshot: PipesStateSnapshot) {
        self.pipes.clear();
        for p in snapshot.pipes {
            self.pipes.insert(p.name.clone(), p);
        }
    }
}
