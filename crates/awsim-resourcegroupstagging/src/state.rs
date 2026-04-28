use std::collections::BTreeMap;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// Per (account, region) state for the Resource Groups Tagging API.
///
/// The real service maintains a cross-service tag index that is populated
/// asynchronously from each tagged resource's owning service. We model that
/// as a flat ARN → tag map; callers wire tags in via `TagResources` /
/// `UntagResources` and read them back via `GetResources` / `GetTagKeys` /
/// `GetTagValues`.
#[derive(Debug, Default)]
pub struct TaggingState {
    pub resources: DashMap<String, BTreeMap<String, String>>,
}

impl TaggingState {
    pub fn snapshot(&self) -> TaggingStateSnapshot {
        TaggingStateSnapshot {
            resources: self
                .resources
                .iter()
                .map(|entry| {
                    (
                        entry.key().clone(),
                        entry
                            .value()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    )
                })
                .collect(),
        }
    }

    pub fn restore(&self, snap: TaggingStateSnapshot) {
        self.resources.clear();
        for (arn, tags) in snap.resources {
            self.resources.insert(arn, tags);
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TaggingStateSnapshot {
    pub resources: BTreeMap<String, BTreeMap<String, String>>,
}
