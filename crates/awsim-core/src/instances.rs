//! Seams between services that act on running EC2 instances.
//!
//! EC2 owns instances and whatever backs them. Systems Manager runs
//! commands on them and reports their agents, and EC2 hands each
//! instance credentials for its instance profile's role, which IAM and
//! STS own. These traits let each service reach the others without a
//! crate dependency, in the same way as [`crate::ParameterLookup`].

use crate::AwsError;

/// Temporary credentials for a role session minted for an instance.
#[derive(Debug, Clone)]
pub struct InstanceCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
    /// ISO 8601 expiry.
    pub expiration: String,
    /// Name of the role the session is for.
    pub role_name: String,
}

/// Mints credentials for the role an instance profile holds, as EC2 does
/// for the instance the profile is attached to.
pub trait InstanceProfileCredentials: Send + Sync {
    /// A fresh session for `instance_profile_arn`'s role, named after the
    /// instance. Fails with `InvalidParameterValue` when the profile does
    /// not exist or holds no role.
    fn issue(
        &self,
        instance_profile_arn: &str,
        instance_id: &str,
        account: &str,
    ) -> Result<InstanceCredentials, AwsError>;
}

/// One instance whose agent has answered, as Systems Manager lists it.
#[derive(Debug, Clone)]
pub struct ManagedInstance {
    pub instance_id: String,
    /// `Online` while the agent answers, `ConnectionLost` otherwise.
    pub ping_status: String,
    /// Unix seconds of the last answer.
    pub last_ping: u64,
    pub ip_address: String,
    pub computer_name: String,
    pub platform_name: String,
    pub platform_version: String,
    /// Name of the instance profile's role, or empty.
    pub iam_role: String,
}

/// How a command started through [`InstanceAgents::run_shell`] ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandExit {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// The agents on instances, through which Systems Manager runs commands.
#[async_trait::async_trait]
pub trait InstanceAgents: Send + Sync {
    /// Every instance in the account and region whose agent has answered.
    fn managed_instances(&self, account: &str, region: &str) -> Vec<ManagedInstance>;

    /// Start `script` under `/bin/sh` as root on the instance and answer a
    /// handle for [`Self::command_exit`]. Fails with `InvalidInstanceId`
    /// when the instance's agent is not online.
    async fn run_shell(
        &self,
        account: &str,
        region: &str,
        instance_id: &str,
        script: &str,
    ) -> Result<i64, AwsError>;

    /// How the command ended, or `None` while it is still running. The
    /// agent forgets a command once its exit has been read, so the caller
    /// keeps the first `Some` it gets.
    async fn command_exit(
        &self,
        account: &str,
        region: &str,
        instance_id: &str,
        handle: i64,
    ) -> Result<Option<CommandExit>, AwsError>;
}
