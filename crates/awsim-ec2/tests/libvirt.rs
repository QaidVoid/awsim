//! Boots a real virtual machine through libvirt, runs SSM commands in it,
//! and checks termination leaves nothing behind.
//!
//! Needs KVM, access to `qemu:///system`, the `default` network, `virsh`,
//! `qemu-img` and an ISO tool. Point `AWSIM_EC2_LIBVIRT_TEST` at a cloud
//! image the `libvirt` user can read, then run:
//!
//! ```text
//! AWSIM_EC2_LIBVIRT_TEST=/path/to/ubuntu-24.04-minimal-cloudimg-amd64.img \
//!     cargo test -p awsim-ec2 --test libvirt -- --ignored --nocapture
//! ```
//!
//! Instance and volume files go in a directory beside the image, which is
//! removed afterwards.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use awsim_core::{
    AwsError, InstanceCredentials, InstanceProfileCredentials, RequestContext, ServiceHandler,
};
use awsim_ec2::Ec2Service;
use awsim_ec2::vm::{Libvirt, VmConfig};
use awsim_ssm::SsmService;
use serde_json::{Value, json};

struct StaticCredentials;

impl InstanceProfileCredentials for StaticCredentials {
    fn issue(&self, _: &str, _: &str, _: &str) -> Result<InstanceCredentials, AwsError> {
        Ok(InstanceCredentials {
            access_key_id: "ASIAAWSIMLIBVIRTTEST".to_string(),
            secret_access_key: "secret".to_string(),
            session_token: "token".to_string(),
            expiration: "2099-01-01T00:00:00Z".to_string(),
            role_name: "test-role".to_string(),
        })
    }
}

fn ctx(service: &str) -> RequestContext {
    RequestContext::new(service, "us-east-1")
}

async fn call(svc: &dyn ServiceHandler, op: &str, input: Value) -> Value {
    svc.handle(op, input, &ctx(svc.service_name()))
        .await
        .unwrap_or_else(|e| panic!("{op} failed: {} {}", e.code, e.message))
}

async fn wait_for<F, Fut>(what: &str, limit: Duration, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let start = Instant::now();
    while !check().await {
        assert!(start.elapsed() < limit, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    eprintln!("{what} after {:?}", start.elapsed());
}

async fn instance_state(ec2: &Ec2Service, id: &str) -> Value {
    let resp = call(ec2, "DescribeInstances", json!({ "InstanceId": [id] })).await;
    resp["reservationSet"]["item"][0]["instancesSet"]["item"][0].clone()
}

async fn run_command(ssm: &SsmService, id: &str, script: &str) -> Value {
    let sent = call(
        ssm,
        "SendCommand",
        json!({
            "DocumentName": "AWS-RunShellScript",
            "InstanceIds": [id],
            "Parameters": { "commands": [script] },
        }),
    )
    .await;
    let command_id = sent["Command"]["CommandId"].as_str().unwrap().to_string();
    let start = Instant::now();
    loop {
        let result = call(
            ssm,
            "GetCommandInvocation",
            json!({ "CommandId": command_id, "InstanceId": id }),
        )
        .await;
        if result["Status"] != "InProgress" {
            return result;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "command never finished"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "boots a real VM; set AWSIM_EC2_LIBVIRT_TEST to a base image"]
async fn boots_runs_a_command_and_terminates_cleanly() {
    let Some(image) = std::env::var_os("AWSIM_EC2_LIBVIRT_TEST").map(PathBuf::from) else {
        eprintln!("AWSIM_EC2_LIBVIRT_TEST is not set; skipping");
        return;
    };
    let image = std::fs::canonicalize(&image).expect("base image");
    let dir = image
        .parent()
        .unwrap()
        .join(format!("awsim-libvirt-test-{}", std::process::id()));

    let libvirt = Libvirt::connect(&dir).await.expect("libvirt");
    let gateway = libvirt.gateway().to_string();
    let config = VmConfig {
        dir: dir.clone(),
        images: vec![("*".to_string(), image)],
        max_vcpus: 1,
        max_memory_mib: 1024,
        max_instances: 1,
        guest_endpoint: None,
    };
    let ec2 = Arc::new(Ec2Service::new().with_machines(
        Arc::new(libvirt),
        config,
        format!("http://{gateway}:4566"),
        gateway,
        Some(Arc::new(StaticCredentials)),
    ));
    let ssm = Arc::new(SsmService::new().with_instance_agents(ec2.instance_agents().unwrap()));

    // The ticker stands in for the server's tick loop.
    let ticker = {
        let ec2 = Arc::clone(&ec2);
        tokio::spawn(async move {
            loop {
                ec2.tick().await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        })
    };

    let user_data = base64_encode(
        b"#cloud-config\nruncmd:\n  - echo booted-by-user-data > /run/awsim-marker\n",
    );
    let launched = call(
        ec2.as_ref(),
        "RunInstances",
        json!({
            "ImageId": "ami-test",
            "InstanceType": "t3.micro",
            "MinCount": 1,
            "MaxCount": 1,
            "UserData": user_data,
            "IamInstanceProfile": { "Arn": "arn:aws:iam::000000000000:instance-profile/test" },
        }),
    )
    .await;
    let id = launched["instancesSet"]["item"][0]["instanceId"]
        .as_str()
        .unwrap()
        .to_string();
    eprintln!("launched {id}");

    // Checks run in their own task so a failed assertion still reaches
    // the termination and cleanup below.
    let checks = {
        let (ec2, ssm, id) = (Arc::clone(&ec2), Arc::clone(&ssm), id.clone());
        tokio::spawn(async move {
            let (ec2, ssm) = (ec2.as_ref(), ssm.as_ref());
            wait_for("instance running", Duration::from_secs(300), || async {
                instance_state(ec2, &id).await["instanceState"]["name"] == "running"
            })
            .await;
            let described = instance_state(ec2, &id).await;
            let address = described["privateIpAddress"].as_str().unwrap().to_string();
            assert!(address.starts_with("192.168."), "lease address {address}");

            let volume = call(
                ec2,
                "CreateVolume",
                json!({ "AvailabilityZone": "us-east-1a", "Size": "1", "VolumeType": "gp3" }),
            )
            .await["volumeId"]
                .as_str()
                .unwrap()
                .to_string();
            call(
                ec2,
                "AttachVolume",
                json!({ "VolumeId": volume, "InstanceId": id, "Device": "/dev/sdh" }),
            )
            .await;

            wait_for("agent online", Duration::from_secs(60), || async {
                let info = call(
                    ssm,
                    "DescribeInstanceInformation",
                    json!({ "Filters": [{ "Key": "InstanceIds", "Values": [id.as_str()] }] }),
                )
                .await;
                info["InstanceInformationList"][0]["PingStatus"] == "Online"
            })
            .await;

            // cloud-init's runcmd runs after the agent may already answer.
            wait_for("user data to run", Duration::from_secs(120), || async {
                run_command(ssm, &id, "cat /run/awsim-marker").await["Status"] == "Success"
            })
            .await;

            let result = run_command(
                ssm,
                &id,
                "id -u; cat /run/awsim-marker; test -b /dev/sdh && echo has-sdh; \
             echo key=$AWS_ACCESS_KEY_ID region=$AWS_REGION; \
             grep -c ASIAAWSIMLIBVIRTTEST /etc/awsim/credentials",
            )
            .await;
            eprintln!("command output: {}", result["StandardOutputContent"]);
            assert_eq!(result["Status"], "Success");
            assert_eq!(result["ResponseCode"], 0);
            let out = result["StandardOutputContent"].as_str().unwrap();
            assert!(out.starts_with("0\n"), "runs as root: {out}");
            assert!(out.contains("booted-by-user-data"), "user data ran: {out}");
            assert!(out.contains("has-sdh"), "volume at /dev/sdh: {out}");
            assert!(
                out.contains("key=ASIAAWSIMLIBVIRTTEST region=us-east-1"),
                "{out}"
            );
            assert!(out.trim_end().ends_with('1'), "credentials file: {out}");

            let failed = run_command(ssm, &id, "echo oops >&2; exit 3").await;
            assert_eq!(failed["Status"], "Failed");
            assert_eq!(failed["ResponseCode"], 3);
            assert_eq!(failed["StandardErrorContent"], "oops\n");

            call(ec2, "DetachVolume", json!({ "VolumeId": volume })).await;
            call(ec2, "DeleteVolume", json!({ "VolumeId": volume })).await;
        })
    };
    let outcome = checks.await;

    call(
        ec2.as_ref(),
        "TerminateInstances",
        json!({ "InstanceId": [id.as_str()] }),
    )
    .await;
    wait_for("instance terminated", Duration::from_secs(30), || async {
        instance_state(&ec2, &id).await["instanceState"]["name"] == "terminated"
    })
    .await;
    ticker.abort();

    let domain = std::process::Command::new("virsh")
        .args(["-c", "qemu:///system", "domstate", &format!("awsim-{id}")])
        .output()
        .unwrap();
    assert!(!domain.status.success(), "domain awsim-{id} still defined");
    assert!(
        !dir.join("instances").join(&id).exists(),
        "instance files left behind"
    );
    let volumes: Vec<_> = std::fs::read_dir(dir.join("volumes")).unwrap().collect();
    assert!(volumes.is_empty(), "volume files left behind");
    std::fs::remove_dir_all(&dir).unwrap();
    if let Err(failure) = outcome {
        std::panic::resume_unwind(failure.into_panic());
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
