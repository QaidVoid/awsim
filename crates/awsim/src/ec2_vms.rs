//! Opt-in libvirt backing for EC2 instances, and the instance profile
//! credentials it hands each guest.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use awsim_core::{AccountRegionStore, AwsError, InstanceCredentials, InstanceProfileCredentials};
use awsim_ec2::vm::{Hypervisor, Libvirt, VmConfig};
use awsim_iam::state::IamState;
use awsim_sts::StsSessionStore;
use tracing::info;

/// Lifetime of an instance's role session. EC2's own instance
/// credentials last about six hours.
const SESSION_SECONDS: u64 = 6 * 3600;

/// A connected backend, ready to hand to the EC2 service.
pub struct Ec2Vms {
    pub hypervisor: Arc<dyn Hypervisor>,
    pub config: VmConfig,
    pub endpoint: String,
    pub gateway: String,
}

/// Read `AWSIM_EC2_*` and connect to libvirt. `None` when the backend is
/// off. Fails startup on a configuration that cannot work.
pub async fn connect(data_dir: Option<&str>, port: u16) -> Result<Option<Ec2Vms>> {
    let Some(config) = VmConfig::from_env(data_dir.map(Path::new)).map_err(|e| anyhow!(e))? else {
        return Ok(None);
    };
    let libvirt = Libvirt::connect(&config.dir)
        .await
        .map_err(|e| anyhow!("AWSIM_EC2_BACKEND=libvirt: {e}"))?;
    let gateway = libvirt.gateway().to_string();
    let endpoint = config
        .guest_endpoint
        .clone()
        .unwrap_or_else(|| format!("http://{gateway}:{port}"));
    info!(
        dir = %config.dir.display(),
        endpoint = %endpoint,
        max_instances = config.max_instances,
        "EC2 instances run as libvirt virtual machines"
    );
    Ok(Some(Ec2Vms {
        hypervisor: Arc::new(libvirt),
        config,
        endpoint,
        gateway,
    }))
}

/// Mints an instance's credentials from IAM's instance profile and role,
/// recorded as an STS session so the gateway resolves them.
pub struct IamInstanceProfileCredentials {
    pub iam: AccountRegionStore<IamState>,
    pub sessions: Arc<StsSessionStore>,
    pub partition: String,
}

impl InstanceProfileCredentials for IamInstanceProfileCredentials {
    fn issue(
        &self,
        instance_profile_arn: &str,
        instance_id: &str,
        account: &str,
    ) -> Result<InstanceCredentials, AwsError> {
        let invalid = || {
            AwsError::bad_request(
                "InvalidParameterValue",
                format!(
                    "Value ({instance_profile_arn}) for parameter iamInstanceProfile.arn is \
                     invalid. Invalid IAM Instance Profile ARN"
                ),
            )
        };
        let name = instance_profile_arn.rsplit('/').next().unwrap_or_default();
        let state = self.iam.get(account, awsim_iam::IAM_REGION);
        let role_name = state
            .instance_profiles
            .get(name)
            .filter(|p| p.arn == instance_profile_arn)
            .and_then(|p| p.roles.first().cloned())
            .ok_or_else(invalid)?;
        let role_arn = state
            .roles
            .get(&role_name)
            .map(|r| r.arn.clone())
            .ok_or_else(invalid)?;
        let credentials = awsim_sts::issue_role_session(
            &self.sessions,
            &self.partition,
            &role_arn,
            instance_id,
            account,
            SESSION_SECONDS,
        );
        let field = |k: &str| credentials[k].as_str().unwrap_or_default().to_string();
        Ok(InstanceCredentials {
            access_key_id: field("AccessKeyId"),
            secret_access_key: field("SecretAccessKey"),
            session_token: field("SessionToken"),
            expiration: field("Expiration"),
            role_name,
        })
    }
}
