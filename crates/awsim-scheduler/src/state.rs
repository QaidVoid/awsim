use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single EventBridge Schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub name: String,
    pub group_name: String,
    pub arn: String,
    pub schedule_expression: String,
    pub target: Value,
    pub flexible_time_window: Value,
    pub state: String,
    pub created_at: u64,
    pub last_modified_at: u64,
}

/// A Schedule Group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleGroup {
    pub name: String,
    pub arn: String,
    pub state: String,
    pub created_at: u64,
}

/// Serializable snapshot.
#[derive(Debug, Serialize, Deserialize)]
pub struct SchedulerStateSnapshot {
    pub schedules: Vec<Schedule>,
    pub schedule_groups: Vec<ScheduleGroup>,
}

/// Per-account/region Scheduler state.
#[derive(Debug, Default)]
pub struct SchedulerState {
    /// "{group}/{name}" → Schedule
    pub schedules: DashMap<String, Schedule>,
    /// name → ScheduleGroup
    pub schedule_groups: DashMap<String, ScheduleGroup>,
}

impl SchedulerState {
    pub fn to_snapshot(&self) -> SchedulerStateSnapshot {
        SchedulerStateSnapshot {
            schedules: self.schedules.iter().map(|e| e.value().clone()).collect(),
            schedule_groups: self
                .schedule_groups
                .iter()
                .map(|e| e.value().clone())
                .collect(),
        }
    }

    pub fn restore_from_snapshot(&self, snapshot: SchedulerStateSnapshot) {
        for schedule in snapshot.schedules {
            let key = format!("{}/{}", schedule.group_name, schedule.name);
            self.schedules.insert(key, schedule);
        }
        for group in snapshot.schedule_groups {
            self.schedule_groups.insert(group.name.clone(), group);
        }
    }
}
