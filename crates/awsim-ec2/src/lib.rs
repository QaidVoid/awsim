mod error;
mod ids;
mod operations;
mod state;
pub mod vm;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, InstanceProfileCredentials, Protocol, RequestContext,
    ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::Ec2State;
use vm::machines::Machines;
use vm::{Hypervisor, VmConfig};

/// The AWSim EC2 service handler.
pub struct Ec2Service {
    store: AccountRegionStore<Ec2State>,
    machines: Option<Arc<Machines>>,
}

impl Ec2Service {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
            machines: None,
        }
    }

    /// Back instances launched from now on with virtual machines.
    ///
    /// `endpoint` is the AWS endpoint written into guests and `gateway`
    /// the host's address on their network. `credentials` mints the
    /// session each instance holds for its instance profile.
    pub fn with_machines(
        mut self,
        hypervisor: Arc<dyn Hypervisor>,
        config: VmConfig,
        endpoint: String,
        gateway: String,
        credentials: Option<Arc<dyn InstanceProfileCredentials>>,
    ) -> Self {
        self.machines = Some(Arc::new(Machines::new(
            hypervisor,
            config,
            self.store.clone(),
            endpoint,
            gateway,
            credentials,
        )));
        self
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<Ec2State> {
        self.store.get(&ctx.account_id, &ctx.region)
    }

    /// The agents of VM-backed instances, for Systems Manager. `None`
    /// without a VM backend.
    pub fn instance_agents(&self) -> Option<Arc<dyn awsim_core::InstanceAgents>> {
        self.machines
            .as_ref()
            .map(|m| Arc::clone(m) as Arc<dyn awsim_core::InstanceAgents>)
    }

    /// Count running instances for a given account+region pair.
    /// Used by the billing meter to charge instance-hours. Stopped /
    /// terminated / pending instances are excluded since AWS doesn't
    /// bill compute time for them.
    pub fn running_instance_count(&self, account_id: &str, region: &str) -> u64 {
        let state = self.store.get(account_id, region);
        state
            .instances
            .iter()
            .filter(|i| i.value().state == "running")
            .count() as u64
    }
}

impl Default for Ec2Service {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for Ec2Service {
    fn service_name(&self) -> &str {
        "ec2"
    }

    fn signing_name(&self) -> &str {
        "ec2"
    }

    fn protocol(&self) -> Protocol {
        Protocol::Ec2Query
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "EC2 request");
        let state = self.get_state(ctx);

        // AWS EC2 implements an opt-in dry-run: any mutating operation
        // that receives DryRun=true must short-circuit with HTTP 412
        // (DryRunOperation) before the state mutates. Read-only Describe*
        // operations ignore the flag.
        if !operation.starts_with("Describe")
            && input.get("DryRun").and_then(Value::as_bool) == Some(true)
        {
            return Err(AwsError::precondition_failed(
                "DryRunOperation",
                format!(
                    "Request would have succeeded, but DryRun flag is set. \
                     Operation: {operation}."
                ),
            ));
        }

        if let Some(machines) = &self.machines {
            match operation {
                "RunInstances" => return machines.run_instances(&state, &input, ctx).await,
                "StartInstances" => return machines.start_instances(&state, &input).await,
                "StopInstances" => return machines.stop_instances(&state, &input).await,
                "TerminateInstances" => {
                    return machines.terminate_instances(&state, &input).await;
                }
                "CreateVolume" => return machines.create_volume(&state, &input, ctx).await,
                "DeleteVolume" => return machines.delete_volume(&state, &input).await,
                "AttachVolume" => return machines.attach_volume(&state, &input).await,
                "DetachVolume" => return machines.detach_volume(&state, &input).await,
                _ => {}
            }
        }

        match operation {
            // VPCs
            "CreateVpc" => operations::vpcs::create_vpc(&state, &input),
            "DeleteVpc" => operations::vpcs::delete_vpc(&state, &input),
            "DescribeVpcs" => operations::vpcs::describe_vpcs(&state, &input),

            // Subnets
            "CreateSubnet" => operations::subnets::create_subnet(&state, &input),
            "DeleteSubnet" => operations::subnets::delete_subnet(&state, &input),
            "DescribeSubnets" => operations::subnets::describe_subnets(&state, &input),

            // Security Groups
            "CreateSecurityGroup" => {
                operations::security_groups::create_security_group(&state, &input)
            }
            "DeleteSecurityGroup" => {
                operations::security_groups::delete_security_group(&state, &input)
            }
            "DescribeSecurityGroups" => {
                operations::security_groups::describe_security_groups(&state, &input)
            }
            "AuthorizeSecurityGroupIngress" => {
                operations::security_groups::authorize_security_group_ingress(&state, &input)
            }
            "AuthorizeSecurityGroupEgress" => {
                operations::security_groups::authorize_security_group_egress(&state, &input)
            }
            "RevokeSecurityGroupIngress" => {
                operations::security_groups::revoke_security_group_ingress(&state, &input)
            }
            "RevokeSecurityGroupEgress" => {
                operations::security_groups::revoke_security_group_egress(&state, &input)
            }

            // Internet Gateways
            "CreateInternetGateway" => {
                operations::gateways::create_internet_gateway(&state, &input)
            }
            "DeleteInternetGateway" => {
                operations::gateways::delete_internet_gateway(&state, &input)
            }
            "AttachInternetGateway" => {
                operations::gateways::attach_internet_gateway(&state, &input)
            }
            "DetachInternetGateway" => {
                operations::gateways::detach_internet_gateway(&state, &input)
            }
            "DescribeInternetGateways" => {
                operations::gateways::describe_internet_gateways(&state, &input)
            }

            // Route Tables
            "CreateRouteTable" => operations::route_tables::create_route_table(&state, &input),
            "DeleteRouteTable" => operations::route_tables::delete_route_table(&state, &input),
            "DescribeRouteTables" => {
                operations::route_tables::describe_route_tables(&state, &input)
            }
            "CreateRoute" => operations::route_tables::create_route(&state, &input),
            "AssociateRouteTable" => {
                operations::route_tables::associate_route_table(&state, &input)
            }

            // Key Pairs
            "CreateKeyPair" => operations::key_pairs::create_key_pair(&state, &input),
            "DeleteKeyPair" => operations::key_pairs::delete_key_pair(&state, &input),
            "DescribeKeyPairs" => operations::key_pairs::describe_key_pairs(&state, &input),

            // Metadata
            "DescribeRegions" => operations::metadata::describe_regions(ctx),
            "DescribeAvailabilityZones" => operations::metadata::describe_availability_zones(ctx),

            // Instances
            "RunInstances" => operations::instances::run_instances(&state, &input, ctx),
            "DescribeInstances" => operations::instances::describe_instances(&state, &input),
            "ModifyInstanceAttribute" => {
                operations::instances::modify_instance_attribute(&state, &input)
            }
            "DescribeInstanceAttribute" => {
                operations::instances::describe_instance_attribute(&state, &input)
            }
            "StartInstances" => operations::instances::start_instances(&state, &input),
            "StopInstances" => operations::instances::stop_instances(&state, &input),
            "RebootInstances" => operations::instances::reboot_instances(&state, &input),
            "TerminateInstances" => operations::instances::terminate_instances(&state, &input),
            "DescribeInstanceStatus" => {
                operations::instances::describe_instance_status(&state, &input)
            }
            "DescribeImages" => operations::instances::describe_images(&state, &input),

            // Volumes
            "CreateVolume" => operations::volumes::create_volume(&state, &input, ctx),
            "DescribeVolumes" => operations::volumes::describe_volumes(&state, &input),
            "AttachVolume" => operations::volumes::attach_volume(&state, &input),
            "DetachVolume" => operations::volumes::detach_volume(&state, &input),
            "DeleteVolume" => operations::volumes::delete_volume(&state, &input),

            // Tags
            "CreateTags" => operations::tags::create_tags(&state, &input),
            "DeleteTags" => operations::tags::delete_tags(&state, &input),
            "DescribeTags" => operations::tags::describe_tags(&state, &input),

            // Stubs (empty-list responses)
            "DescribeNetworkInterfaces" => {
                operations::stubs::describe_network_interfaces(&state, &input)
            }
            "DescribeNatGateways" => operations::stubs::describe_nat_gateways(&state, &input),
            "DescribeVpcEndpoints" => operations::stubs::describe_vpc_endpoints(&state, &input),
            "DescribeAddresses" => operations::stubs::describe_addresses(&state, &input),

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    async fn tick(&self) {
        if let Some(machines) = &self.machines {
            machines.kick();
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        awsim_core::snapshot_store(&self.store)
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        awsim_core::restore_store(&self.store, data)
    }
}

#[cfg(test)]
mod dry_run_tests {
    use super::*;
    use awsim_core::ServiceHandler;
    use serde_json::json;

    fn ctx() -> RequestContext {
        RequestContext::new("ec2", "us-east-1")
    }

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
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => {}
            }
        }
    }

    #[test]
    fn write_op_with_dry_run_short_circuits_with_dryrunoperation() {
        let svc = Ec2Service::new();
        let err = block_on(svc.handle(
            "RunInstances",
            json!({ "ImageId": "ami-12345678", "MinCount": 1, "MaxCount": 1, "DryRun": true }),
            &ctx(),
        ))
        .unwrap_err();
        assert_eq!(err.code, "DryRunOperation");
        assert_eq!(err.status.as_u16(), 412);
    }

    #[test]
    fn read_op_ignores_dry_run() {
        let svc = Ec2Service::new();
        block_on(svc.handle("DescribeInstances", json!({ "DryRun": true }), &ctx())).unwrap();
    }

    #[test]
    fn write_op_without_dry_run_still_runs() {
        let svc = Ec2Service::new();
        let resp = block_on(svc.handle(
            "RunInstances",
            json!({ "ImageId": "ami-12345678", "MinCount": 1, "MaxCount": 1 }),
            &ctx(),
        ))
        .unwrap();
        assert!(resp.get("instancesSet").is_some() || resp.get("InstancesSet").is_some());
    }
}
