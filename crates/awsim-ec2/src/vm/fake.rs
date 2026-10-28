use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use async_trait::async_trait;
use awsim_core::CommandExit;
use dashmap::DashMap;

use super::hypervisor::{GuestInfo, Hypervisor, MachineSpec, Power, VmResult};

#[derive(Debug, Clone)]
struct Domain {
    spec: MachineSpec,
    power: Power,
    vcpus: u32,
    memory_mib: u64,
    volumes: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
struct Exec {
    instance_id: String,
    script: String,
    stdin: String,
    polls: u32,
}

/// A hypervisor that keeps machines in memory. Guests boot the moment
/// they start, and a command finishes on its second status read, echoing
/// its script, with the exit code given by a trailing `exit N`.
#[derive(Default)]
pub struct FakeHypervisor {
    domains: DashMap<String, Domain>,
    volumes: DashMap<String, u64>,
    calls: DashMap<&'static str, usize>,
    execs: DashMap<i64, Exec>,
    next_pid: AtomicI64,
    agent_down: AtomicBool,
    start_fails: AtomicBool,
}

impl FakeHypervisor {
    fn call(&self, name: &'static str) {
        *self.calls.entry(name).or_default() += 1;
    }

    pub fn calls_to(&self, name: &str) -> usize {
        self.calls.get(name).map(|c| *c).unwrap_or(0)
    }

    pub fn set_agent(&self, answers: bool) {
        self.agent_down.store(!answers, Ordering::SeqCst);
    }

    pub fn fail_start(&self, fail: bool) {
        self.start_fails.store(fail, Ordering::SeqCst);
    }

    pub fn spec(&self, id: &str) -> Option<MachineSpec> {
        self.domains.get(id).map(|d| d.spec.clone())
    }

    pub fn power_of(&self, id: &str) -> Option<Power> {
        self.domains.get(id).map(|d| d.power)
    }

    pub fn shape_of(&self, id: &str) -> Option<(u32, u64)> {
        self.domains.get(id).map(|d| (d.vcpus, d.memory_mib))
    }

    pub fn domain_count(&self) -> usize {
        self.domains.len()
    }

    /// The guest powers itself off.
    pub fn power_off(&self, id: &str) {
        if let Some(mut d) = self.domains.get_mut(id) {
            d.power = Power::Off;
        }
    }

    /// The machine disappears behind AWSim's back.
    pub fn forget(&self, id: &str) {
        self.domains.remove(id);
    }

    /// The last command run on a machine, and what it was fed.
    pub fn last_exec(&self, id: &str) -> Option<(String, String)> {
        self.execs
            .iter()
            .filter(|e| e.instance_id == id)
            .max_by_key(|e| *e.key())
            .map(|e| (e.script.clone(), e.stdin.clone()))
    }

    fn on(&self, id: &str) -> bool {
        self.domains.get(id).is_some_and(|d| d.power == Power::On)
    }
}

#[async_trait]
impl Hypervisor for FakeHypervisor {
    async fn create(&self, spec: &MachineSpec) -> VmResult<()> {
        self.call("create");
        self.domains.insert(
            spec.instance_id.clone(),
            Domain {
                spec: spec.clone(),
                power: Power::Off,
                vcpus: spec.vcpus,
                memory_mib: spec.memory_mib,
                volumes: Vec::new(),
            },
        );
        Ok(())
    }

    async fn start(&self, id: &str) -> VmResult<()> {
        self.call("start");
        if self.start_fails.load(Ordering::SeqCst) {
            return Err("start failed".into());
        }
        let mut d = self.domains.get_mut(id).ok_or("no such domain")?;
        d.power = Power::On;
        Ok(())
    }

    async fn shutdown(&self, _id: &str) -> VmResult<()> {
        self.call("shutdown");
        Ok(())
    }

    async fn destroy(&self, id: &str) -> VmResult<()> {
        self.call("destroy");
        self.power_off(id);
        Ok(())
    }

    async fn remove(&self, id: &str) -> VmResult<()> {
        self.call("remove");
        self.domains.remove(id);
        Ok(())
    }

    async fn power(&self, id: &str) -> VmResult<Option<Power>> {
        Ok(self.power_of(id))
    }

    async fn resize(&self, id: &str, vcpus: u32, memory_mib: u64) -> VmResult<()> {
        self.call("resize");
        let mut d = self.domains.get_mut(id).ok_or("no such domain")?;
        d.vcpus = vcpus;
        d.memory_mib = memory_mib;
        Ok(())
    }

    async fn create_volume(&self, volume_id: &str, size_gib: u64) -> VmResult<()> {
        self.call("create_volume");
        self.volumes.insert(volume_id.to_string(), size_gib);
        Ok(())
    }

    async fn delete_volume(&self, volume_id: &str) -> VmResult<()> {
        self.call("delete_volume");
        self.volumes.remove(volume_id);
        Ok(())
    }

    async fn attach_volume(&self, id: &str, volume_id: &str, serial: &str) -> VmResult<()> {
        self.call("attach_volume");
        let mut d = self.domains.get_mut(id).ok_or("no such domain")?;
        d.volumes.push((volume_id.to_string(), serial.to_string()));
        Ok(())
    }

    async fn detach_volume(&self, id: &str, volume_id: &str) -> VmResult<()> {
        self.call("detach_volume");
        let mut d = self.domains.get_mut(id).ok_or("no such domain")?;
        d.volumes.retain(|(v, _)| v != volume_id);
        Ok(())
    }

    async fn ping(&self, id: &str) -> bool {
        self.on(id) && !self.agent_down.load(Ordering::SeqCst)
    }

    async fn guest_info(&self, id: &str) -> Option<GuestInfo> {
        self.on(id).then(|| GuestInfo {
            os_name: "Ubuntu".to_string(),
            os_version: "24.04".to_string(),
            hostname: id.to_string(),
        })
    }

    async fn address(&self, id: &str) -> Option<String> {
        self.on(id).then(|| "192.168.122.50".to_string())
    }

    async fn exec(
        &self,
        id: &str,
        script: &str,
        _env: &[String],
        stdin: Option<&[u8]>,
    ) -> VmResult<i64> {
        self.call("exec");
        if !self.ping(id).await {
            return Err("Guest agent is not responding".into());
        }
        let pid = self.next_pid.fetch_add(1, Ordering::SeqCst) + 1000;
        self.execs.insert(
            pid,
            Exec {
                instance_id: id.to_string(),
                script: script.to_string(),
                stdin: String::from_utf8_lossy(stdin.unwrap_or_default()).into_owned(),
                polls: 0,
            },
        );
        Ok(pid)
    }

    async fn exec_status(&self, _id: &str, pid: i64) -> VmResult<Option<CommandExit>> {
        let mut exec = self.execs.get_mut(&pid).ok_or("Invalid parameter 'pid'")?;
        exec.polls += 1;
        if exec.polls < 2 && exec.stdin.is_empty() {
            return Ok(None);
        }
        let exit_code = exec
            .script
            .trim_end()
            .rsplit_once("exit ")
            .and_then(|(_, n)| n.parse().ok())
            .unwrap_or(0);
        Ok(Some(CommandExit {
            exit_code,
            stdout: exec.script.clone(),
            stderr: if exit_code == 0 {
                String::new()
            } else {
                "failed\n".to_string()
            },
        }))
    }
}
