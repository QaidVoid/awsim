use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{MessageMoveTask, SqsState};

/// Token-bucket rate limiter used by message-move tasks to enforce
/// `MaxNumberOfMessagesPerSecond`. The bucket refills at the configured
/// rate up to the same capacity (1 second's worth of tokens) so a
/// short burst can drain the bucket but a sustained move loop is
/// capped at the per-second rate. AWS sizes the bucket the same way.
///
/// Wired into the actual redrive loop once the move task gains a
/// background runner; until then it's exercised by the unit tests
/// below to lock in the AWS-documented semantics.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MessageMoveRateLimiter {
    capacity: f64,
    refill_per_sec: f64,
    tokens: f64,
    last_refill_secs: f64,
}

#[allow(dead_code)]
impl MessageMoveRateLimiter {
    pub fn new(rate_per_sec: u32) -> Self {
        let capacity = f64::from(rate_per_sec.max(1));
        Self {
            capacity,
            refill_per_sec: capacity,
            tokens: capacity,
            last_refill_secs: 0.0,
        }
    }

    /// Attempt to consume one token at `now_secs` (monotonic seconds
    /// since some shared epoch). Returns `true` when a token was
    /// available; `false` otherwise. The caller is expected to back
    /// off (sleep until the next refill, retry) when refused.
    pub fn try_acquire(&mut self, now_secs: f64) -> bool {
        let elapsed = (now_secs - self.last_refill_secs).max(0.0);
        if elapsed > 0.0 {
            self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
            self.last_refill_secs = now_secs;
        }
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// StartMessageMoveTask — begin a DLQ redrive task (stub).
pub fn start_message_move_task(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let source_arn = input["SourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "SourceArn is required"))?;
    let destination_arn = input["DestinationArn"].as_str().map(str::to_string);

    // AWS bounds MaxNumberOfMessagesPerSecond at 1..=500; outside-range
    // values come back as InvalidParameterValue. None leaves the move
    // loop uncapped (the AWS-managed default).
    let max_messages_per_second = match input["MaxNumberOfMessagesPerSecond"].as_i64() {
        Some(n) if !(1..=500).contains(&n) => {
            return Err(AwsError::bad_request(
                "InvalidParameterValue",
                format!("MaxNumberOfMessagesPerSecond {n} must be between 1 and 500."),
            ));
        }
        Some(n) => Some(n as u32),
        None => None,
    };

    let task_handle = Uuid::new_v4().to_string();

    let task = MessageMoveTask {
        task_handle: task_handle.clone(),
        source_arn: source_arn.to_string(),
        destination_arn,
        status: "RUNNING".to_string(),
        started_timestamp: now_secs(),
        approximate_number_of_messages_moved: 0,
        approximate_number_of_messages_to_move: 0,
        max_messages_per_second,
    };

    state.move_tasks.insert(task_handle.clone(), task);

    Ok(json!({ "TaskHandle": task_handle }))
}

/// CancelMessageMoveTask — cancel a running DLQ redrive task (stub).
pub fn cancel_message_move_task(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let task_handle = input["TaskHandle"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "TaskHandle is required"))?;

    let mut task = state.move_tasks.get_mut(task_handle).ok_or_else(|| {
        AwsError::not_found(
            "ResourceNotFoundException",
            format!("Task not found: {task_handle}"),
        )
    })?;

    if task.status == "COMPLETED" || task.status == "CANCELLED" {
        return Err(AwsError::bad_request(
            "InvalidParameterValue",
            format!(
                "Task {} is already in terminal state {}",
                task_handle, task.status
            ),
        ));
    }

    let moved = task.approximate_number_of_messages_moved;
    task.status = "CANCELLED".to_string();

    Ok(json!({ "ApproximateNumberOfMessagesMoved": moved }))
}

/// ListMessageMoveTasks — list move tasks for a source ARN.
pub fn list_message_move_tasks(
    state: &SqsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let source_arn = input["SourceArn"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "SourceArn is required"))?;

    let results: Vec<Value> = state
        .move_tasks
        .iter()
        .filter(|entry| entry.value().source_arn == source_arn)
        .map(|entry| {
            let t = entry.value();
            let mut obj = json!({
                "TaskHandle": t.task_handle,
                "SourceArn": t.source_arn,
                "Status": t.status,
                "StartedTimestamp": t.started_timestamp,
                "ApproximateNumberOfMessagesMoved": t.approximate_number_of_messages_moved,
                "ApproximateNumberOfMessagesToMove": t.approximate_number_of_messages_to_move,
            });
            if let Some(dst) = &t.destination_arn {
                obj["DestinationArn"] = Value::String(dst.clone());
            }
            obj
        })
        .collect();

    Ok(json!({ "Results": results }))
}

#[cfg(test)]
mod rate_limiter_tests {
    use super::*;

    #[test]
    fn first_request_succeeds_when_bucket_is_full() {
        let mut bucket = MessageMoveRateLimiter::new(10);
        assert!(bucket.try_acquire(0.0));
    }

    #[test]
    fn refuses_after_burst_drains_capacity() {
        let mut bucket = MessageMoveRateLimiter::new(3);
        for _ in 0..3 {
            assert!(bucket.try_acquire(0.0));
        }
        assert!(!bucket.try_acquire(0.0));
    }

    #[test]
    fn refills_proportionally_to_elapsed_time() {
        let mut bucket = MessageMoveRateLimiter::new(10);
        // Drain.
        for _ in 0..10 {
            bucket.try_acquire(0.0);
        }
        // Half a second elapsed -> 5 tokens refilled.
        assert!(bucket.try_acquire(0.5));
        assert!(bucket.try_acquire(0.5));
        assert!(bucket.try_acquire(0.5));
        assert!(bucket.try_acquire(0.5));
        assert!(bucket.try_acquire(0.5));
        // Sixth pull at the same instant should fail.
        assert!(!bucket.try_acquire(0.5));
    }

    #[test]
    fn cap_is_one_second_of_refill_so_burst_does_not_grow() {
        let mut bucket = MessageMoveRateLimiter::new(5);
        // Idle for an hour — bucket should be capped at 5, not 18000.
        for _ in 0..5 {
            assert!(bucket.try_acquire(3600.0));
        }
        assert!(!bucket.try_acquire(3600.0));
    }

    #[test]
    fn sustained_rate_matches_configured_per_second() {
        let mut bucket = MessageMoveRateLimiter::new(10);
        let mut allowed = 0;
        let mut now = 0.0;
        // Run for 1 second of simulated time, asking every 50 ms.
        while now <= 1.0 {
            if bucket.try_acquire(now) {
                allowed += 1;
            }
            now += 0.05;
        }
        // Allow initial-burst capacity (10) plus one second of refill
        // (another 10) — total bounded near 20.
        assert!((10..=21).contains(&allowed), "allowed={allowed}");
    }
}
