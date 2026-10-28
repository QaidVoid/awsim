use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, CommandExit, InstanceAgents, InstanceCredentials,
    InstanceProfileCredentials, ManagedInstance, RequestContext,
};
use dashmap::DashMap;
use serde_json::{Value, json};
use tracing::{info, warn};

use super::VmConfig;
use super::hypervisor::{GuestInfo, Hypervisor, MachineSpec, Power};
use super::seed::{self, GUEST_CREDENTIALS, GuestAws};
use crate::ids::{new_ec2_id, now_iso8601};
use crate::operations::instances::{
    LaunchRequest, collect_id_param, instance_to_value, launched_with_token, new_instance,
    release_volumes,
};
use crate::operations::volumes::{
    CreatePlan, commit_attach_volume, commit_create_volume, commit_delete_volume,
    commit_detach_volume, device_serial, plan_attach_volume, plan_create_volume,
    plan_delete_volume, plan_detach_volume,
};
use crate::state::{Ec2State, Instance};

/// Seconds between reconcile passes.
const RECONCILE_EVERY: u64 = 2;
/// Seconds between agent pings of a running instance.
const PING_EVERY: u64 = 10;
/// An agent is `Online` while its last answer is at most this old.
const ONLINE_WITHIN: u64 = 30;
/// Seconds a graceful stop gets before the machine is powered off.
const STOP_GRACE: u64 = 300;
/// Credentials older than this are replaced.
const CREDENTIAL_REFRESH: u64 = 3600;

/// What AWSim knows about a guest that is not EC2 state, and so is not
/// persisted: it is rebuilt by watching the machine.
#[derive(Debug, Clone, Default)]
pub(crate) struct Agent {
    /// Unix seconds of the last answer to a ping.
    pub last_answer: Option<u64>,
    pub last_ping: u64,
    pub info: Option<GuestInfo>,
    pub credentials: Option<(u64, InstanceCredentials)>,
    pub stop_requested: Option<u64>,
}

/// The libvirt backend: launches, stops and removes the machines behind
/// VM-backed instances, and watches them.
pub struct Machines {
    pub(crate) hv: Arc<dyn Hypervisor>,
    pub(crate) config: VmConfig,
    pub(crate) store: AccountRegionStore<Ec2State>,
    endpoint: String,
    gateway: String,
    credentials: Option<Arc<dyn InstanceProfileCredentials>>,
    pub(crate) agents: DashMap<String, Agent>,
    reconciling: AtomicBool,
    last_pass: AtomicU64,
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn instance_not_found(id: &str) -> AwsError {
    AwsError::bad_request(
        "InvalidInstanceID.NotFound",
        format!("The instance ID '{id}' does not exist"),
    )
}

/// EC2's own spelling of a server-side failure.
fn internal_error(message: String) -> AwsError {
    AwsError::server_error("InternalError", message)
}

fn state_change(id: &str, previous: &str, current: &str) -> Value {
    json!({
        "instanceId": id,
        "currentState": { "code": state_code(current), "name": current },
        "previousState": { "code": state_code(previous), "name": previous },
    })
}

fn state_code(name: &str) -> u32 {
    match name {
        "pending" => 0,
        "running" => 16,
        "shutting-down" => 32,
        "terminated" => 48,
        "stopping" => 64,
        "stopped" => 80,
        _ => 0,
    }
}

/// States in which an instance's machine holds memory.
fn holds_memory(state: &str) -> bool {
    matches!(state, "pending" | "running" | "stopping")
}

impl Machines {
    pub(crate) fn new(
        hv: Arc<dyn Hypervisor>,
        config: VmConfig,
        store: AccountRegionStore<Ec2State>,
        endpoint: String,
        gateway: String,
        credentials: Option<Arc<dyn InstanceProfileCredentials>>,
    ) -> Self {
        Self {
            hv,
            config,
            store,
            endpoint,
            gateway,
            credentials,
            agents: DashMap::new(),
            reconciling: AtomicBool::new(false),
            last_pass: AtomicU64::new(0),
        }
    }

    pub(crate) fn guest_aws(&self, region: &str) -> GuestAws {
        GuestAws {
            endpoint: self.endpoint.clone(),
            region: region.to_string(),
            gateway: self.gateway.clone(),
        }
    }

    fn issue_credentials(
        &self,
        profile: Option<&str>,
        instance_id: &str,
        account: &str,
    ) -> Result<Option<InstanceCredentials>, AwsError> {
        match (profile, &self.credentials) {
            (Some(arn), Some(issuer)) => issuer.issue(arn, instance_id, account).map(Some),
            _ => Ok(None),
        }
    }

    fn running_machines(&self) -> usize {
        self.store
            .iter_all()
            .iter()
            .map(|(_, state)| {
                state
                    .instances
                    .iter()
                    .filter(|i| i.vm && holds_memory(&i.state))
                    .count()
            })
            .sum()
    }

    fn capacity_error(&self, instance_type: &str, zone: &str) -> AwsError {
        AwsError::server_error(
            "InsufficientInstanceCapacity",
            format!(
                "We currently do not have sufficient {instance_type} capacity in the Availability \
                 Zone you requested ({zone}). AWSim runs at most {} virtual machines at once \
                 (AWSIM_EC2_VM_MAX_INSTANCES).",
                self.config.max_instances
            ),
        )
    }

    // -----------------------------------------------------------------------
    // RunInstances
    // -----------------------------------------------------------------------

    pub(crate) async fn run_instances(
        &self,
        state: &Ec2State,
        input: &Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        let launch = LaunchRequest::parse(input, ctx)?;
        if let Some(token) = &launch.client_token
            && let Some(previous) = launched_with_token(state, token)
        {
            return Ok(previous);
        }
        let image = self
            .config
            .image_for(&launch.image_id)
            .filter(|p| p.is_file())
            .map(|p| p.to_path_buf())
            .ok_or_else(|| {
                AwsError::bad_request(
                    "InvalidAMIID.NotFound",
                    format!("The image id '[{}]' does not exist", launch.image_id),
                )
            })?;
        if let Some(data) = &launch.user_data {
            seed::decode_user_data(data)
                .map_err(|e| AwsError::bad_request("InvalidParameterValue", e))?;
        }
        if self.running_machines() + launch.count as usize > self.config.max_instances {
            return Err(self.capacity_error(&launch.instance_type, &format!("{}a", ctx.region)));
        }
        let (vcpus, memory_mib) = self.config.shape(&launch.instance_type);
        let reservation_id = new_ec2_id("r");

        let mut items = Vec::new();
        for _ in 0..launch.count {
            let mut instance = new_instance(state, &launch, &reservation_id, "pending", ctx);
            instance.vm = true;
            instance.private_ip_address = None;
            let id = instance.instance_id.clone();

            let launched = async {
                let credentials = self.issue_credentials(
                    instance.iam_instance_profile.as_deref(),
                    &id,
                    &ctx.account_id,
                )?;
                let user_data = seed::user_data(
                    launch.user_data.as_deref(),
                    &self.guest_aws(&ctx.region),
                    credentials.as_ref(),
                )
                .map_err(|e| AwsError::bad_request("InvalidParameterValue", e))?;
                let spec = MachineSpec {
                    instance_id: id.clone(),
                    base_image: image.clone(),
                    root_gib: launch.root_size_gib,
                    vcpus,
                    memory_mib,
                    user_data,
                    meta_data: seed::meta_data(&id),
                };
                self.hv
                    .create(&spec)
                    .await
                    .map_err(|e| internal_error(format!("cannot create {id}: {e}")))?;
                self.hv
                    .start(&id)
                    .await
                    .map_err(|e| internal_error(format!("cannot start {id}: {e}")))?;
                Ok::<_, AwsError>(credentials)
            }
            .await;

            match launched {
                Ok(credentials) => {
                    self.agents.insert(
                        id.clone(),
                        Agent {
                            credentials: credentials.map(|c| (now_secs(), c)),
                            ..Agent::default()
                        },
                    );
                    info!(instance_id = %id, vcpus, memory_mib, "RunInstances (libvirt)");
                    items.push(instance_to_value(&instance));
                    state.instances.insert(id, instance);
                }
                Err(e) => {
                    if let Err(cleanup) = self.hv.remove(&id).await {
                        warn!(instance_id = %id, error = %cleanup, "cleanup after failed launch");
                    }
                    for device in &instance.block_devices {
                        state.volumes.remove(&device.volume_id);
                        state.resource_tags.remove(&device.volume_id);
                    }
                    state.resource_tags.remove(&id);
                    return Err(e);
                }
            }
        }

        Ok(json!({
            "instancesSet": { "item": items },
            "reservationId": reservation_id,
            "ownerId": "000000000000",
        }))
    }

    // -----------------------------------------------------------------------
    // StartInstances / StopInstances / TerminateInstances
    // -----------------------------------------------------------------------

    fn snapshot(state: &Ec2State, ids: &[String]) -> Result<Vec<Instance>, AwsError> {
        ids.iter()
            .map(|id| {
                state
                    .instances
                    .get(id)
                    .map(|i| i.value().clone())
                    .ok_or_else(|| instance_not_found(id))
            })
            .collect()
    }

    /// Move an instance from `from` to `to`, unless a request moved it
    /// elsewhere while the machine was being looked at. Answers whether
    /// it moved.
    fn advance(state: &Ec2State, id: &str, from: &str, to: &str, reason: &str) -> bool {
        let Some(mut instance) = state.instances.get_mut(id) else {
            return false;
        };
        if instance.state != from {
            return false;
        }
        instance.previous_state = Some(instance.state.clone());
        instance.state = to.to_string();
        instance.state_transition_reason = reason.to_string();
        true
    }

    fn set_state(state: &Ec2State, id: &str, to: &str, reason: &str) {
        if let Some(mut instance) = state.instances.get_mut(id) {
            instance.previous_state = Some(instance.state.clone());
            instance.state = to.to_string();
            instance.state_transition_reason = reason.to_string();
        }
    }

    pub(crate) async fn start_instances(
        &self,
        state: &Ec2State,
        input: &Value,
    ) -> Result<Value, AwsError> {
        let instances = Self::snapshot(state, &collect_id_param(input, "InstanceId"))?;
        for i in &instances {
            if !matches!(i.state.as_str(), "stopped" | "pending" | "running") {
                return Err(AwsError::bad_request(
                    "IncorrectInstanceState",
                    format!(
                        "The instance '{}' is not in a state from which it can be started.",
                        i.instance_id
                    ),
                ));
            }
        }
        let starting = instances.iter().filter(|i| i.state == "stopped").count();
        if starting > 0 && self.running_machines() + starting > self.config.max_instances {
            return Err(
                self.capacity_error(&instances[0].instance_type, &instances[0].availability_zone)
            );
        }

        let mut changes = Vec::new();
        for i in &instances {
            if i.state != "stopped" {
                changes.push(state_change(&i.instance_id, &i.state, &i.state));
                continue;
            }
            if i.vm {
                let (vcpus, memory_mib) = self.config.shape(&i.instance_type);
                self.hv
                    .resize(&i.instance_id, vcpus, memory_mib)
                    .await
                    .and(self.hv.start(&i.instance_id).await)
                    .map_err(|e| internal_error(format!("cannot start {}: {e}", i.instance_id)))?;
                self.agents
                    .entry(i.instance_id.clone())
                    .or_default()
                    .stop_requested = None;
                Self::set_state(state, &i.instance_id, "pending", "");
            } else {
                Self::set_state(state, &i.instance_id, "running", "");
            }
            if let Some(mut instance) = state.instances.get_mut(&i.instance_id) {
                instance.launch_time = now_iso8601();
                if instance.vm {
                    instance.private_ip_address = None;
                }
            }
            let now = if i.vm { "pending" } else { "running" };
            changes.push(state_change(&i.instance_id, "stopped", now));
        }
        Ok(json!({ "instancesSet": { "item": changes } }))
    }

    pub(crate) async fn stop_instances(
        &self,
        state: &Ec2State,
        input: &Value,
    ) -> Result<Value, AwsError> {
        let instances = Self::snapshot(state, &collect_id_param(input, "InstanceId"))?;
        for i in &instances {
            if matches!(i.state.as_str(), "terminated" | "shutting-down") {
                return Err(AwsError::bad_request(
                    "IncorrectInstanceState",
                    format!(
                        "The instance '{}' is not in a state from which it can be stopped.",
                        i.instance_id
                    ),
                ));
            }
        }
        let force =
            input["Force"].as_bool() == Some(true) || input["Force"].as_str() == Some("true");
        let mut changes = Vec::new();
        for i in &instances {
            if !matches!(i.state.as_str(), "running" | "pending") {
                changes.push(state_change(&i.instance_id, &i.state, &i.state));
                continue;
            }
            if !i.vm {
                Self::set_state(state, &i.instance_id, "stopped", "User initiated");
                changes.push(state_change(&i.instance_id, &i.state, "stopped"));
                continue;
            }
            let stopped = if force {
                self.hv.destroy(&i.instance_id).await
            } else {
                self.hv.shutdown(&i.instance_id).await
            };
            stopped.map_err(|e| internal_error(format!("cannot stop {}: {e}", i.instance_id)))?;
            self.agents
                .entry(i.instance_id.clone())
                .or_default()
                .stop_requested = Some(now_secs());
            Self::set_state(state, &i.instance_id, "stopping", "User initiated");
            changes.push(state_change(&i.instance_id, &i.state, "stopping"));
        }
        Ok(json!({ "instancesSet": { "item": changes } }))
    }

    pub(crate) async fn terminate_instances(
        &self,
        state: &Ec2State,
        input: &Value,
    ) -> Result<Value, AwsError> {
        let instances = Self::snapshot(state, &collect_id_param(input, "InstanceId"))?;
        let mut changes = Vec::new();
        for i in &instances {
            if matches!(i.state.as_str(), "terminated" | "shutting-down") {
                changes.push(state_change(&i.instance_id, &i.state, &i.state));
                continue;
            }
            if i.vm {
                self.hv.remove(&i.instance_id).await.map_err(|e| {
                    internal_error(format!("cannot terminate {}: {e}", i.instance_id))
                })?;
                self.agents.remove(&i.instance_id);
            }
            release_volumes(state, &i.instance_id);
            let now = if i.vm { "shutting-down" } else { "terminated" };
            Self::set_state(state, &i.instance_id, now, "Client.UserInitiatedShutdown");
            if let Some(mut instance) = state.instances.get_mut(&i.instance_id) {
                instance.private_ip_address = None;
            }
            info!(instance_id = %i.instance_id, "TerminateInstances (libvirt)");
            changes.push(state_change(&i.instance_id, &i.state, now));
        }
        Ok(json!({ "instancesSet": { "item": changes } }))
    }

    // -----------------------------------------------------------------------
    // Volumes
    // -----------------------------------------------------------------------

    pub(crate) async fn create_volume(
        &self,
        state: &Ec2State,
        input: &Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        match plan_create_volume(state, input, ctx)? {
            CreatePlan::Existing(value) => Ok(value),
            CreatePlan::New(volume) => {
                self.hv
                    .create_volume(&volume.volume_id, volume.size_gib)
                    .await
                    .map_err(|e| {
                        internal_error(format!("cannot create {}: {e}", volume.volume_id))
                    })?;
                Ok(commit_create_volume(state, *volume))
            }
        }
    }

    pub(crate) async fn delete_volume(
        &self,
        state: &Ec2State,
        input: &Value,
    ) -> Result<Value, AwsError> {
        let volume_id = plan_delete_volume(state, input)?;
        self.hv
            .delete_volume(&volume_id)
            .await
            .map_err(|e| internal_error(format!("cannot delete {volume_id}: {e}")))?;
        Ok(commit_delete_volume(state, &volume_id))
    }

    pub(crate) async fn attach_volume(
        &self,
        state: &Ec2State,
        input: &Value,
    ) -> Result<Value, AwsError> {
        let plan = plan_attach_volume(state, input)?;
        if plan.vm {
            let serial = device_serial(&plan.device).unwrap_or_default();
            self.hv
                .attach_volume(&plan.instance_id, &plan.volume_id, serial)
                .await
                .map_err(|e| internal_error(format!("cannot attach {}: {e}", plan.volume_id)))?;
        }
        Ok(commit_attach_volume(state, &plan))
    }

    pub(crate) async fn detach_volume(
        &self,
        state: &Ec2State,
        input: &Value,
    ) -> Result<Value, AwsError> {
        let plan = plan_detach_volume(state, input)?;
        if plan.vm {
            self.hv
                .detach_volume(&plan.instance_id, &plan.volume_id)
                .await
                .map_err(|e| internal_error(format!("cannot detach {}: {e}", plan.volume_id)))?;
        }
        Ok(commit_detach_volume(state, &plan))
    }

    // -----------------------------------------------------------------------
    // Reconcile
    // -----------------------------------------------------------------------

    /// Start a reconcile pass in the background unless one is running or
    /// the last one was too recent. Called from the service's `tick`.
    pub(crate) fn kick(self: &Arc<Self>) {
        let now = now_secs();
        if now.saturating_sub(self.last_pass.load(Ordering::Relaxed)) < RECONCILE_EVERY {
            return;
        }
        if self.reconciling.swap(true, Ordering::AcqRel) {
            return;
        }
        let machines = Arc::clone(self);
        tokio::spawn(async move {
            machines.reconcile().await;
            machines.last_pass.store(now_secs(), Ordering::Relaxed);
            machines.reconciling.store(false, Ordering::Release);
        });
    }

    /// Move every VM-backed instance toward what its machine is doing.
    pub(crate) async fn reconcile(&self) {
        for ((account, region), state) in self.store.iter_all() {
            let watched: Vec<Instance> = state
                .instances
                .iter()
                .filter(|i| i.vm && i.state != "terminated")
                .map(|i| i.value().clone())
                .collect();
            for instance in watched {
                self.reconcile_one(&state, &account, &region, &instance)
                    .await;
            }
        }
    }

    async fn reconcile_one(&self, state: &Ec2State, account: &str, region: &str, i: &Instance) {
        let id = i.instance_id.as_str();
        let from = i.state.as_str();
        if from == "shutting-down" {
            Self::advance(
                state,
                id,
                from,
                "terminated",
                "Client.UserInitiatedShutdown",
            );
            return;
        }
        let power = match self.hv.power(id).await {
            Ok(power) => power,
            Err(e) => {
                warn!(instance_id = id, error = %e, "cannot read machine state");
                return;
            }
        };
        let Some(power) = power else {
            warn!(
                instance_id = id,
                "machine is gone; marking the instance terminated"
            );
            if Self::advance(state, id, from, "terminated", "Server.InternalError") {
                self.agents.remove(id);
                release_volumes(state, id);
            }
            return;
        };
        let now = now_secs();

        match (i.state.as_str(), power) {
            ("pending" | "running", Power::Off) => {
                let reason = if i.state == "running" {
                    "Client.InstanceInitiatedShutdown"
                } else {
                    "Server.InternalError"
                };
                Self::advance(state, id, from, "stopped", reason);
            }
            ("stopping", Power::Off) => {
                Self::advance(state, id, from, "stopped", "User initiated");
            }
            ("stopping", Power::On) => {
                let asked = self.agents.get(id).and_then(|a| a.stop_requested);
                if asked.is_some_and(|t| now.saturating_sub(t) > STOP_GRACE) {
                    warn!(instance_id = id, "guest ignored shutdown; powering off");
                    if let Err(e) = self.hv.destroy(id).await {
                        warn!(instance_id = id, error = %e, "cannot power off");
                    }
                }
            }
            ("stopped", Power::On) => {
                Self::advance(state, id, from, "pending", "");
            }
            ("pending", Power::On) => {
                if !self.hv.ping(id).await {
                    return;
                }
                let Some(address) = self.hv.address(id).await else {
                    return;
                };
                let info = self.hv.guest_info(id).await;
                {
                    let mut agent = self.agents.entry(id.to_string()).or_default();
                    agent.last_answer = Some(now);
                    agent.last_ping = now;
                    agent.info = info;
                }
                if !Self::advance(state, id, from, "running", "") {
                    return;
                }
                if let Some(mut instance) = state.instances.get_mut(id) {
                    instance.private_ip_address = Some(address);
                }
                info!(instance_id = id, "instance running");
                self.refresh_credentials(i, account, region, false).await;
            }
            ("running", Power::On) => {
                let due = self
                    .agents
                    .get(id)
                    .is_none_or(|a| now.saturating_sub(a.last_ping) >= PING_EVERY);
                if due {
                    let answered = self.hv.ping(id).await;
                    let mut agent = self.agents.entry(id.to_string()).or_default();
                    agent.last_ping = now;
                    if answered {
                        agent.last_answer = Some(now);
                    }
                }
                self.refresh_credentials(i, account, region, false).await;
            }
            _ => {}
        }
    }

    /// The instance's current credentials, minting and pushing new ones
    /// into the guest when they are missing or old, or when `force`.
    pub(crate) async fn refresh_credentials(
        &self,
        i: &Instance,
        account: &str,
        region: &str,
        force: bool,
    ) -> Option<InstanceCredentials> {
        let profile = i.iam_instance_profile.as_deref()?;
        let now = now_secs();
        let current = self
            .agents
            .get(&i.instance_id)
            .and_then(|a| a.credentials.clone());
        if let Some((minted, credentials)) = &current
            && !force
            && now.saturating_sub(*minted) < CREDENTIAL_REFRESH
        {
            return Some(credentials.clone());
        }
        let credentials = match self.issue_credentials(Some(profile), &i.instance_id, account) {
            Ok(c) => c?,
            Err(e) => {
                warn!(instance_id = %i.instance_id, error = %e.message, "cannot mint credentials");
                return current.map(|c| c.1);
            }
        };
        let file = seed::credentials_file(&credentials);
        let script = format!(
            "umask 022 && mkdir -p /etc/awsim && cat > {GUEST_CREDENTIALS}.new && mv {GUEST_CREDENTIALS}.new {GUEST_CREDENTIALS}"
        );
        let env = self.guest_aws(region).command_env(None);
        match self
            .hv
            .exec(&i.instance_id, &script, &env, Some(file.as_bytes()))
            .await
        {
            Ok(pid) => {
                // Read the exit so the agent can forget the process.
                for _ in 0..10 {
                    if let Ok(Some(_)) | Err(_) = self.hv.exec_status(&i.instance_id, pid).await {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                self.agents
                    .entry(i.instance_id.clone())
                    .or_default()
                    .credentials = Some((now, credentials.clone()));
                Some(credentials)
            }
            Err(e) => {
                warn!(instance_id = %i.instance_id, error = %e, "cannot write credentials");
                current.map(|c| c.1)
            }
        }
    }
}

fn not_managed(instance_id: &str, account: &str) -> AwsError {
    AwsError::bad_request(
        "InvalidInstanceId",
        format!("Instances [[{instance_id}]] not in a valid state for account {account}"),
    )
}

#[async_trait]
impl InstanceAgents for Machines {
    fn managed_instances(&self, account: &str, region: &str) -> Vec<ManagedInstance> {
        let now = now_secs();
        let state = self.store.get(account, region);
        let mut found: Vec<ManagedInstance> = state
            .instances
            .iter()
            .filter(|i| i.vm && !matches!(i.state.as_str(), "terminated" | "shutting-down"))
            .filter_map(|i| {
                let agent = self.agents.get(&i.instance_id)?;
                let last = agent.last_answer?;
                let online = i.state == "running" && now.saturating_sub(last) <= ONLINE_WITHIN;
                let info = agent.info.clone().unwrap_or_default();
                Some(ManagedInstance {
                    instance_id: i.instance_id.clone(),
                    ping_status: if online { "Online" } else { "ConnectionLost" }.to_string(),
                    last_ping: last,
                    ip_address: i.private_ip_address.clone().unwrap_or_default(),
                    computer_name: info.hostname,
                    platform_name: info.os_name,
                    platform_version: info.os_version,
                    iam_role: agent
                        .credentials
                        .as_ref()
                        .map(|c| c.1.role_name.clone())
                        .unwrap_or_default(),
                })
            })
            .collect();
        found.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
        found
    }

    async fn run_shell(
        &self,
        account: &str,
        region: &str,
        instance_id: &str,
        script: &str,
    ) -> Result<i64, AwsError> {
        let online = self
            .managed_instances(account, region)
            .into_iter()
            .any(|m| m.instance_id == instance_id && m.ping_status == "Online");
        let instance = self
            .store
            .get(account, region)
            .instances
            .get(instance_id)
            .map(|i| i.value().clone());
        let Some(instance) = instance.filter(|_| online) else {
            return Err(not_managed(instance_id, account));
        };
        let credentials = self
            .refresh_credentials(&instance, account, region, false)
            .await;
        let env = self.guest_aws(region).command_env(credentials.as_ref());
        self.hv
            .exec(instance_id, script, &env, None)
            .await
            .map_err(|e| {
                warn!(instance_id, error = %e, "cannot start command");
                not_managed(instance_id, account)
            })
    }

    async fn command_exit(
        &self,
        _account: &str,
        _region: &str,
        instance_id: &str,
        handle: i64,
    ) -> Result<Option<CommandExit>, AwsError> {
        self.hv
            .exec_status(instance_id, handle)
            .await
            .map_err(|e| internal_error(format!("cannot read command on {instance_id}: {e}")))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::vm::fake::FakeHypervisor;
    use std::path::PathBuf;

    pub(crate) struct FakeCredentials;

    impl InstanceProfileCredentials for FakeCredentials {
        fn issue(
            &self,
            arn: &str,
            instance_id: &str,
            _account: &str,
        ) -> Result<InstanceCredentials, AwsError> {
            if arn.ends_with("/missing") {
                return Err(AwsError::bad_request("InvalidParameterValue", "no profile"));
            }
            Ok(InstanceCredentials {
                access_key_id: format!("ASIA{}", now_secs()),
                secret_access_key: "secret".to_string(),
                session_token: format!("token-{instance_id}"),
                expiration: "2099-01-01T00:00:00Z".to_string(),
                role_name: "owner".to_string(),
            })
        }
    }

    pub(crate) fn machines(fake: Arc<FakeHypervisor>, max_instances: usize) -> Arc<Machines> {
        let image = std::env::temp_dir().join("awsim-ec2-fake-base.img");
        std::fs::write(&image, b"base").unwrap();
        let config = VmConfig {
            dir: PathBuf::from("/nonexistent"),
            images: vec![("*".to_string(), image)],
            max_vcpus: 2,
            max_memory_mib: 2048,
            max_instances,
            guest_endpoint: None,
        };
        Arc::new(Machines::new(
            fake,
            config,
            AccountRegionStore::new(),
            "http://192.168.122.1:4566".to_string(),
            "192.168.122.1".to_string(),
            Some(Arc::new(FakeCredentials)),
        ))
    }

    pub(crate) fn ctx() -> RequestContext {
        RequestContext::new("ec2", "us-east-1")
    }

    pub(crate) async fn launch(m: &Machines, extra: Value) -> String {
        let state = m.store.get("000000000000", "us-east-1");
        let mut input = json!({
            "ImageId": "ami-ws",
            "InstanceType": "t3.large",
            "MinCount": 1,
            "MaxCount": 1,
            "IamInstanceProfile": { "Arn": "arn:aws:iam::000000000000:instance-profile/ws" },
        });
        for (k, v) in extra.as_object().unwrap() {
            input[k] = v.clone();
        }
        let resp = m.run_instances(&state, &input, &ctx()).await.unwrap();
        assert_eq!(
            resp["instancesSet"]["item"][0]["instanceState"]["name"],
            "pending"
        );
        resp["instancesSet"]["item"][0]["instanceId"]
            .as_str()
            .unwrap()
            .to_string()
    }

    pub(crate) fn instance_state(m: &Machines, id: &str) -> Instance {
        m.store
            .get("000000000000", "us-east-1")
            .instances
            .get(id)
            .unwrap()
            .clone()
    }

    #[tokio::test]
    async fn launch_creates_a_machine_from_the_mapped_image() {
        let fake = Arc::new(FakeHypervisor::default());
        let m = machines(fake.clone(), 2);
        let id = launch(&m, json!({ "UserData": "IyEvYmluL3NoCmVjaG8gaGkK" })).await;
        let spec = fake.spec(&id).unwrap();
        assert_eq!((spec.vcpus, spec.memory_mib, spec.root_gib), (2, 2048, 8));
        assert!(spec.base_image.ends_with("awsim-ec2-fake-base.img"));
        let user_data = String::from_utf8(spec.user_data).unwrap();
        assert!(
            user_data.contains("aws_session_token = token-"),
            "seed carries credentials"
        );
        assert!(spec.meta_data.contains(&format!("instance-id: {id}")));
        assert_eq!(fake.power_of(&id), Some(Power::On));
        let i = instance_state(&m, &id);
        assert!(i.vm);
        assert_eq!(i.state, "pending");
        assert!(i.private_ip_address.is_none());
    }

    #[tokio::test]
    async fn pending_becomes_running_when_the_agent_answers() {
        let fake = Arc::new(FakeHypervisor::default());
        fake.set_agent(false);
        let m = machines(fake.clone(), 2);
        let id = launch(&m, json!({})).await;
        m.reconcile().await;
        assert_eq!(instance_state(&m, &id).state, "pending");

        fake.set_agent(true);
        m.reconcile().await;
        let i = instance_state(&m, &id);
        assert_eq!(i.state, "running");
        assert_eq!(i.private_ip_address.as_deref(), Some("192.168.122.50"));
        assert!(m.agents.get(&id).unwrap().last_answer.is_some());
    }

    #[tokio::test]
    async fn unmapped_or_missing_image_is_not_found() {
        let fake = Arc::new(FakeHypervisor::default());
        let m = machines(fake.clone(), 2);
        let mut config = m.config.clone();
        config.images = vec![("ami-only".to_string(), PathBuf::from("/nope.img"))];
        let m = Machines::new(
            fake.clone(),
            config,
            AccountRegionStore::new(),
            String::new(),
            String::new(),
            None,
        );
        let state = m.store.get("000000000000", "us-east-1");
        for ami in ["ami-other", "ami-only"] {
            let err = m
                .run_instances(
                    &state,
                    &json!({ "ImageId": ami, "MinCount": 1, "MaxCount": 1 }),
                    &ctx(),
                )
                .await
                .unwrap_err();
            assert_eq!(err.code, "InvalidAMIID.NotFound");
        }
        assert!(state.instances.is_empty());
        assert!(state.volumes.is_empty());
        assert_eq!(fake.domain_count(), 0);
    }

    #[tokio::test]
    async fn failed_launch_leaves_nothing_behind() {
        let fake = Arc::new(FakeHypervisor::default());
        fake.fail_start(true);
        let m = machines(fake.clone(), 2);
        let state = m.store.get("000000000000", "us-east-1");
        let err = m
            .run_instances(&state, &json!({ "MinCount": 1, "MaxCount": 1 }), &ctx())
            .await
            .unwrap_err();
        assert_eq!(err.code, "InternalError");
        assert!(state.instances.is_empty());
        assert!(state.volumes.is_empty());
        assert_eq!(fake.domain_count(), 0);
    }

    #[tokio::test]
    async fn unknown_instance_profile_refuses_the_launch() {
        let fake = Arc::new(FakeHypervisor::default());
        let m = machines(fake.clone(), 2);
        let state = m.store.get("000000000000", "us-east-1");
        let err = m
            .run_instances(
                &state,
                &json!({
                    "MinCount": 1, "MaxCount": 1,
                    "IamInstanceProfile": { "Arn": "arn:aws:iam::000000000000:instance-profile/missing" },
                }),
                &ctx(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
        assert_eq!(fake.domain_count(), 0);
    }

    #[tokio::test]
    async fn capacity_cap_refuses_launch_and_start() {
        let fake = Arc::new(FakeHypervisor::default());
        let m = machines(fake.clone(), 1);
        let state = m.store.get("000000000000", "us-east-1");
        let first = launch(&m, json!({})).await;
        let err = m
            .run_instances(&state, &json!({ "MinCount": 1, "MaxCount": 1 }), &ctx())
            .await
            .unwrap_err();
        assert_eq!(err.code, "InsufficientInstanceCapacity");

        m.stop_instances(&state, &json!({ "InstanceId": [first.clone()] }))
            .await
            .unwrap();
        fake.power_off(&first);
        m.reconcile().await;
        let second = launch(&m, json!({})).await;
        let err = m
            .start_instances(&state, &json!({ "InstanceId": [first] }))
            .await
            .unwrap_err();
        assert_eq!(err.code, "InsufficientInstanceCapacity");
        let _ = second;
    }

    #[tokio::test]
    async fn stop_start_cycle_follows_the_machine() {
        let fake = Arc::new(FakeHypervisor::default());
        let m = machines(fake.clone(), 2);
        let state = m.store.get("000000000000", "us-east-1");
        let id = launch(&m, json!({})).await;
        m.reconcile().await;
        assert_eq!(instance_state(&m, &id).state, "running");

        let resp = m
            .stop_instances(&state, &json!({ "InstanceId": [id.clone()] }))
            .await
            .unwrap();
        assert_eq!(
            resp["instancesSet"]["item"][0]["currentState"]["name"],
            "stopping"
        );
        assert_eq!(fake.calls_to("shutdown"), 1);
        m.reconcile().await;
        assert_eq!(instance_state(&m, &id).state, "stopping");
        fake.power_off(&id);
        m.reconcile().await;
        assert_eq!(instance_state(&m, &id).state, "stopped");

        // Stopping a stopped instance changes nothing.
        let again = m
            .stop_instances(&state, &json!({ "InstanceId": [id.clone()] }))
            .await
            .unwrap();
        assert_eq!(
            again["instancesSet"]["item"][0]["currentState"]["name"],
            "stopped"
        );

        let resp = m
            .start_instances(&state, &json!({ "InstanceId": [id.clone()] }))
            .await
            .unwrap();
        assert_eq!(
            resp["instancesSet"]["item"][0]["currentState"]["name"],
            "pending"
        );
        assert_eq!(fake.calls_to("resize"), 1);
        m.reconcile().await;
        assert_eq!(instance_state(&m, &id).state, "running");
    }

    #[tokio::test]
    async fn resize_applies_on_next_start() {
        let fake = Arc::new(FakeHypervisor::default());
        let m = machines(fake.clone(), 2);
        let state = m.store.get("000000000000", "us-east-1");
        let id = launch(&m, json!({ "InstanceType": "t3.nano" })).await;
        assert_eq!(fake.spec(&id).unwrap().memory_mib, 512);
        m.stop_instances(&state, &json!({ "InstanceId": [id.clone()] }))
            .await
            .unwrap();
        fake.power_off(&id);
        m.reconcile().await;
        crate::operations::instances::modify_instance_attribute(
            &state,
            &json!({ "InstanceId": id, "InstanceType": { "Value": "t3.micro" } }),
        )
        .unwrap();
        m.start_instances(&state, &json!({ "InstanceId": [id.clone()] }))
            .await
            .unwrap();
        assert_eq!(fake.shape_of(&id), Some((2, 1024)));
    }

    #[tokio::test]
    async fn guest_power_off_reads_as_stopped() {
        let fake = Arc::new(FakeHypervisor::default());
        let m = machines(fake.clone(), 2);
        let id = launch(&m, json!({})).await;
        m.reconcile().await;
        fake.power_off(&id);
        m.reconcile().await;
        let i = instance_state(&m, &id);
        assert_eq!(i.state, "stopped");
        assert_eq!(
            i.state_transition_reason,
            "Client.InstanceInitiatedShutdown"
        );
    }

    #[tokio::test]
    async fn ignored_shutdown_is_forced_after_the_grace_period() {
        let fake = Arc::new(FakeHypervisor::default());
        let m = machines(fake.clone(), 2);
        let state = m.store.get("000000000000", "us-east-1");
        let id = launch(&m, json!({})).await;
        m.reconcile().await;
        m.stop_instances(&state, &json!({ "InstanceId": [id.clone()] }))
            .await
            .unwrap();
        m.agents.get_mut(&id).unwrap().stop_requested = Some(now_secs() - STOP_GRACE - 1);
        m.reconcile().await;
        assert_eq!(fake.calls_to("destroy"), 1);
        m.reconcile().await;
        assert_eq!(instance_state(&m, &id).state, "stopped");
    }

    #[tokio::test]
    async fn terminate_removes_only_that_machine() {
        let fake = Arc::new(FakeHypervisor::default());
        let m = machines(fake.clone(), 2);
        let state = m.store.get("000000000000", "us-east-1");
        let keep = launch(&m, json!({})).await;
        let gone = launch(&m, json!({})).await;
        let root = instance_state(&m, &gone).block_devices[0].volume_id.clone();

        let resp = m
            .terminate_instances(&state, &json!({ "InstanceId": [gone.clone()] }))
            .await
            .unwrap();
        assert_eq!(
            resp["instancesSet"]["item"][0]["currentState"]["name"],
            "shutting-down"
        );
        assert!(fake.power_of(&gone).is_none());
        assert_eq!(fake.power_of(&keep), Some(Power::On));
        assert!(!state.volumes.contains_key(&root));
        m.reconcile().await;
        assert_eq!(instance_state(&m, &gone).state, "terminated");

        let again = m
            .terminate_instances(&state, &json!({ "InstanceId": [gone] }))
            .await
            .unwrap();
        assert_eq!(
            again["instancesSet"]["item"][0]["currentState"]["name"],
            "terminated"
        );
        let err = m
            .terminate_instances(&state, &json!({ "InstanceId": ["i-00000000"] }))
            .await
            .unwrap_err();
        assert_eq!(err.code, "InvalidInstanceID.NotFound");
    }

    #[tokio::test]
    async fn vanished_machine_reads_as_terminated() {
        let fake = Arc::new(FakeHypervisor::default());
        let m = machines(fake.clone(), 2);
        let id = launch(&m, json!({})).await;
        fake.forget(&id);
        m.reconcile().await;
        assert_eq!(instance_state(&m, &id).state, "terminated");
    }

    #[tokio::test]
    async fn stale_credentials_are_pushed_into_the_guest() {
        let fake = Arc::new(FakeHypervisor::default());
        let m = machines(fake.clone(), 2);
        let id = launch(&m, json!({})).await;
        m.reconcile().await;
        assert_eq!(fake.calls_to("exec"), 0, "fresh seed credentials are kept");
        if let Some((minted, _)) = m.agents.get_mut(&id).unwrap().credentials.as_mut() {
            *minted -= CREDENTIAL_REFRESH + 1;
        }
        m.reconcile().await;
        assert_eq!(fake.calls_to("exec"), 1);
        let (script, stdin) = fake.last_exec(&id).unwrap();
        assert!(script.contains(GUEST_CREDENTIALS));
        assert!(stdin.contains("aws_session_token = token-"));
    }

    #[tokio::test]
    async fn volumes_are_files_hot_plugged_by_device_name() {
        let fake = Arc::new(FakeHypervisor::default());
        let m = machines(fake.clone(), 2);
        let state = m.store.get("000000000000", "us-east-1");
        let id = launch(&m, json!({})).await;
        m.reconcile().await;

        let created = m
            .create_volume(
                &state,
                &json!({ "AvailabilityZone": "us-east-1a", "Size": "1", "VolumeType": "gp3" }),
                &ctx(),
            )
            .await
            .unwrap();
        let volume = created["volumeId"].as_str().unwrap().to_string();
        assert!(fake.volume_exists(&volume));

        m.attach_volume(
            &state,
            &json!({ "VolumeId": volume, "InstanceId": id, "Device": "/dev/sdh" }),
        )
        .await
        .unwrap();
        assert_eq!(
            fake.attached(&id),
            vec![(volume.clone(), "sdh".to_string())]
        );
        assert_eq!(state.volumes.get(&volume).unwrap().state, "in-use");

        let err = m
            .delete_volume(&state, &json!({ "VolumeId": volume }))
            .await
            .unwrap_err();
        assert_eq!(err.code, "VolumeInUse");
        assert!(fake.volume_exists(&volume));

        m.detach_volume(&state, &json!({ "VolumeId": volume }))
            .await
            .unwrap();
        assert!(fake.attached(&id).is_empty());
        m.delete_volume(&state, &json!({ "VolumeId": volume }))
            .await
            .unwrap();
        assert!(!fake.volume_exists(&volume));
    }

    #[tokio::test]
    async fn terminate_keeps_attached_volume_files() {
        let fake = Arc::new(FakeHypervisor::default());
        let m = machines(fake.clone(), 2);
        let state = m.store.get("000000000000", "us-east-1");
        let id = launch(&m, json!({})).await;
        m.reconcile().await;
        let created = m
            .create_volume(
                &state,
                &json!({ "AvailabilityZone": "us-east-1a", "Size": "1" }),
                &ctx(),
            )
            .await
            .unwrap();
        let volume = created["volumeId"].as_str().unwrap().to_string();
        m.attach_volume(
            &state,
            &json!({ "VolumeId": volume, "InstanceId": id, "Device": "/dev/sdh" }),
        )
        .await
        .unwrap();
        m.terminate_instances(&state, &json!({ "InstanceId": [id] }))
            .await
            .unwrap();
        assert!(fake.volume_exists(&volume));
        assert_eq!(state.volumes.get(&volume).unwrap().state, "available");
    }

    #[tokio::test]
    async fn agents_report_presence_and_run_commands_with_credentials() {
        let fake = Arc::new(FakeHypervisor::default());
        fake.set_agent(false);
        let m = machines(fake.clone(), 2);
        let state = m.store.get("000000000000", "us-east-1");
        let id = launch(&m, json!({})).await;
        m.reconcile().await;
        assert!(m.managed_instances("000000000000", "us-east-1").is_empty());
        let err = m
            .run_shell("000000000000", "us-east-1", &id, "true")
            .await
            .unwrap_err();
        assert_eq!(err.code, "InvalidInstanceId");

        fake.set_agent(true);
        m.reconcile().await;
        let listed = m.managed_instances("000000000000", "us-east-1");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].ping_status, "Online");
        assert_eq!(listed[0].ip_address, "192.168.122.50");
        assert_eq!(listed[0].platform_name, "Ubuntu");
        assert_eq!(listed[0].iam_role, "owner");

        let pid = m
            .run_shell("000000000000", "us-east-1", &id, "echo hi; exit 3")
            .await
            .unwrap();
        let env = fake.last_env(&id);
        assert!(
            env.iter()
                .any(|e| e.starts_with("AWS_SESSION_TOKEN=token-"))
        );
        assert!(env.contains(&"AWS_REGION=us-east-1".to_string()));
        assert!(env.contains(&"AWS_ENDPOINT_URL=http://192.168.122.1:4566".to_string()));
        assert_eq!(
            m.command_exit("000000000000", "us-east-1", &id, pid)
                .await
                .unwrap(),
            None
        );
        let exit = m
            .command_exit("000000000000", "us-east-1", &id, pid)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(exit.exit_code, 3);

        m.stop_instances(&state, &json!({ "InstanceId": [id.clone()] }))
            .await
            .unwrap();
        let listed = m.managed_instances("000000000000", "us-east-1");
        assert_eq!(listed[0].ping_status, "ConnectionLost");
        m.terminate_instances(&state, &json!({ "InstanceId": [id] }))
            .await
            .unwrap();
        assert!(m.managed_instances("000000000000", "us-east-1").is_empty());
    }

    #[tokio::test]
    async fn reconcile_never_overwrites_a_newer_request() {
        let fake = Arc::new(FakeHypervisor::default());
        let m = machines(fake.clone(), 2);
        let state = m.store.get("000000000000", "us-east-1");
        let id = launch(&m, json!({})).await;
        // The pass saw `pending`, and a terminate lands before it writes.
        let seen = instance_state(&m, &id);
        m.terminate_instances(&state, &json!({ "InstanceId": [id.clone()] }))
            .await
            .unwrap();
        m.reconcile_one(&state, "000000000000", "us-east-1", &seen)
            .await;
        let i = instance_state(&m, &id);
        assert_eq!(i.state, "shutting-down");
        assert_eq!(i.state_transition_reason, "Client.UserInitiatedShutdown");
    }
}
