use std::path::PathBuf;

use async_trait::async_trait;
use awsim_core::CommandExit;

/// A failed hypervisor call, as the message the tool printed.
pub type VmResult<T> = Result<T, String>;

/// Everything needed to create one instance's machine.
#[derive(Debug, Clone)]
pub struct MachineSpec {
    pub instance_id: String,
    pub base_image: PathBuf,
    pub root_gib: u64,
    pub vcpus: u32,
    pub memory_mib: u64,
    /// The complete cloud-init `user-data` document.
    pub user_data: Vec<u8>,
    /// The cloud-init `meta-data` document.
    pub meta_data: String,
}

/// Whether a machine is consuming CPU and memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Power {
    On,
    Off,
}

/// What the guest says about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuestInfo {
    pub os_name: String,
    pub os_version: String,
    pub hostname: String,
}

/// The one seam between EC2 and whatever runs its machines.
///
/// Every call names an instance or volume by its EC2 id, and the
/// implementation derives its own names from that, so nothing a caller
/// passes can address a machine or file AWSim did not create.
#[async_trait]
pub trait Hypervisor: Send + Sync {
    /// Create the root disk and seed and define the machine, powered off.
    async fn create(&self, spec: &MachineSpec) -> VmResult<()>;
    async fn start(&self, instance_id: &str) -> VmResult<()>;
    /// Ask the guest to power off.
    async fn shutdown(&self, instance_id: &str) -> VmResult<()>;
    /// Power off at once. A machine already off is not an error.
    async fn destroy(&self, instance_id: &str) -> VmResult<()>;
    /// Undefine the machine and delete its own files. Attached volumes
    /// are left alone. A machine already gone is not an error.
    async fn remove(&self, instance_id: &str) -> VmResult<()>;
    /// `None` when no such machine is defined.
    async fn power(&self, instance_id: &str) -> VmResult<Option<Power>>;
    /// Change the CPUs and memory the machine has from its next start.
    async fn resize(&self, instance_id: &str, vcpus: u32, memory_mib: u64) -> VmResult<()>;
    async fn create_volume(&self, volume_id: &str, size_gib: u64) -> VmResult<()>;
    async fn delete_volume(&self, volume_id: &str) -> VmResult<()>;
    /// Attach a volume, live too when the machine is on. `serial` is what
    /// the guest's udev rule names the device after.
    async fn attach_volume(&self, instance_id: &str, volume_id: &str, serial: &str)
    -> VmResult<()>;
    async fn detach_volume(&self, instance_id: &str, volume_id: &str) -> VmResult<()>;
    /// Whether the guest agent answers.
    async fn ping(&self, instance_id: &str) -> bool;
    async fn guest_info(&self, instance_id: &str) -> Option<GuestInfo>;
    /// The guest's IPv4 address from the DHCP lease.
    async fn address(&self, instance_id: &str) -> Option<String>;
    /// Start `/bin/sh -c script` as root with exactly `env`, feeding
    /// `stdin`, and answer the guest process id.
    async fn exec(
        &self,
        instance_id: &str,
        script: &str,
        env: &[String],
        stdin: Option<&[u8]>,
    ) -> VmResult<i64>;
    /// `None` while the process runs.
    async fn exec_status(&self, instance_id: &str, pid: i64) -> VmResult<Option<CommandExit>>;
}
