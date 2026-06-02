//! Broker lifecycle: a delayed `CreateBroker` parks the broker in
//! `CREATION_IN_PROGRESS` until its transition deadline elapses, at
//! which point `tick` (or a `DescribeBroker` poll) promotes it to
//! `RUNNING`. Also guards that `DescribeUser` never surfaces a
//! password.

use awsim_core::{RequestContext, ServiceHandler};
use awsim_mq::MqService;
use serde_json::{Value, json};

/// Minimal executor: these handlers never yield, so a busy-poll
/// resolves the future immediately without pulling in a runtime.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop_clone(_: *const ()) -> RawWaker {
        noop_raw_waker()
    }
    fn noop(_: *const ()) {}
    fn noop_raw_waker() -> RawWaker {
        static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
    let mut cx = Context::from_waker(&waker);
    let mut fut = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

fn ctx() -> RequestContext {
    RequestContext::new("mq", "us-east-1")
}

#[test]
fn broker_creates_in_progress_then_ticks_to_running() {
    // Force a non-zero create delay so the broker is observably
    // transitional before promotion. The default (0) would promote on
    // the first describe; the explicit delay exercises the state
    // machine.
    unsafe {
        std::env::set_var("AWSIM_MQ_CREATE_DELAY_SECS", "0.2");
    }

    let svc = MqService::new();
    let ctx = ctx();
    let created = block_on(svc.handle(
        "CreateBroker",
        json!({
            "BrokerName": "lifecycle",
            "EngineType": "RABBITMQ",
            "EngineVersion": "3.13",
            "HostInstanceType": "mq.m5.large",
            "Users": [{ "Username": "admin", "ConsoleAccess": true, "Password": "s3cr3t" }],
        }),
        &ctx,
    ))
    .unwrap();
    let id = created["BrokerId"].as_str().unwrap().to_string();

    // Immediately after create the broker is still spinning up. The
    // describe poll must NOT promote it yet because the deadline lies
    // in the future.
    let early = describe(&svc, &ctx, &id);
    assert_eq!(
        early["BrokerState"], "CREATION_IN_PROGRESS",
        "broker must be transitional before its deadline elapses"
    );

    // Wait out the deadline, then drive the tick loop. After the
    // deadline passes a single tick flips the broker to RUNNING.
    std::thread::sleep(std::time::Duration::from_millis(250));
    block_on(svc.tick());

    let settled = describe(&svc, &ctx, &id);
    assert_eq!(
        settled["BrokerState"], "RUNNING",
        "tick must promote the broker once its deadline elapses"
    );

    // DescribeUser must never surface the password (plaintext or
    // hashed) regardless of lifecycle state.
    let user = block_on(svc.handle(
        "DescribeUser",
        json!({ "BrokerId": id, "Username": "admin" }),
        &ctx,
    ))
    .unwrap();
    let serialized = user.to_string();
    assert!(
        !serialized.contains("s3cr3t"),
        "plaintext password leaked: {serialized}"
    );
    assert!(
        user.get("Password").is_none(),
        "Password field must be absent: {serialized}"
    );

    // With the delay cleared, the default zero-delay path promotes a
    // fresh broker on the very first describe poll, no tick required.
    // Asserting this in the same test keeps the process-global env var
    // from racing a sibling test.
    unsafe {
        std::env::remove_var("AWSIM_MQ_CREATE_DELAY_SECS");
    }
    let instant = block_on(svc.handle(
        "CreateBroker",
        json!({
            "BrokerName": "instant",
            "EngineType": "RABBITMQ",
            "EngineVersion": "3.13",
            "HostInstanceType": "mq.t3.micro",
        }),
        &ctx,
    ))
    .unwrap();
    let instant_id = instant["BrokerId"].as_str().unwrap().to_string();
    assert_eq!(
        describe(&svc, &ctx, &instant_id)["BrokerState"],
        "RUNNING",
        "zero-delay broker must promote on the first describe poll"
    );
}

fn describe(svc: &MqService, ctx: &RequestContext, id: &str) -> Value {
    block_on(svc.handle("DescribeBroker", json!({ "BrokerId": id }), ctx)).unwrap()
}
