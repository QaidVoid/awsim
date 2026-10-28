use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// EC2 state. Per account+region.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Ec2State {
    pub vpcs: DashMap<String, Vpc>,
    pub subnets: DashMap<String, Subnet>,
    pub security_groups: DashMap<String, SecurityGroup>,
    pub internet_gateways: DashMap<String, InternetGateway>,
    pub route_tables: DashMap<String, RouteTable>,
    pub key_pairs: DashMap<String, KeyPair>,
    /// instanceId -> Instance
    pub instances: DashMap<String, Instance>,
    /// Elastic IP allocation id -> Address
    pub addresses: DashMap<String, Address>,
    /// resource-specific tags: resource_id -> (key -> value)
    pub resource_tags: DashMap<String, HashMap<String, String>>,
    /// Per-subnet host-octet cursor for the next launched instance. Real EC2
    /// allocates from the subnet's CIDR; we just bump a counter starting
    /// at host .10 (real EC2 reserves the first 4 addresses anyway).
    pub subnet_next_host: DashMap<String, u32>,
    /// volumeId -> Volume, including the root volume of every instance.
    #[serde(default)]
    pub volumes: DashMap<String, Volume>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vpc {
    pub vpc_id: String,
    pub cidr_block: String,
    pub state: String,
    pub is_default: bool,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subnet {
    pub subnet_id: String,
    pub vpc_id: String,
    pub cidr_block: String,
    pub availability_zone: String,
    pub state: String,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityGroup {
    pub group_id: String,
    pub group_name: String,
    pub description: String,
    pub vpc_id: String,
    pub ip_permissions: Vec<IpPermission>,
    pub ip_permissions_egress: Vec<IpPermission>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpPermission {
    pub from_port: Option<i64>,
    pub to_port: Option<i64>,
    pub ip_protocol: String,
    pub ip_ranges: Vec<IpRange>,
    /// References to other security groups. Each entry is
    /// `{ GroupId, UserId? }`. AWS allows SGs to authorize traffic
    /// from other SGs, including cross-account when UserId is set.
    pub user_id_group_pairs: Vec<UserIdGroupPair>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserIdGroupPair {
    pub group_id: String,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpRange {
    pub cidr_ip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternetGateway {
    pub internet_gateway_id: String,
    /// VPC ID if attached, None if detached
    pub attached_vpc_id: Option<String>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteTable {
    pub route_table_id: String,
    pub vpc_id: String,
    pub routes: Vec<Route>,
    /// subnet_id -> association_id
    pub associations: HashMap<String, String>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub destination_cidr_block: String,
    pub gateway_id: Option<String>,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPair {
    pub key_name: String,
    pub key_fingerprint: String,
    pub create_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub instance_id: String,
    pub instance_type: String,
    pub image_id: String,
    /// EC2 lifecycle state. `pending` | `running` | `stopping` | `stopped`
    /// | `shutting-down` | `terminated`. Real EC2 transitions through these
    /// asynchronously; we move synchronously since there's nothing to wait
    /// on, but keep the state machine valid (e.g. you can't Start a
    /// `terminated` instance).
    pub state: String,
    pub previous_state: Option<String>,
    pub state_transition_reason: String,
    pub subnet_id: Option<String>,
    pub vpc_id: Option<String>,
    pub private_ip_address: Option<String>,
    pub launch_time: String,
    /// All instances from a single RunInstances batch share a reservation.
    /// DescribeInstances groups them under one reservationSet entry.
    pub reservation_id: String,
    pub tags: HashMap<String, String>,
    /// UserData blob supplied at launch. AWS persists it as base64 and
    /// echoes it from `DescribeInstanceAttribute --attribute userData`.
    pub user_data: Option<String>,
    /// Availability zone, taken from the launch subnet when it is known.
    #[serde(default = "default_availability_zone")]
    pub availability_zone: String,
    #[serde(default)]
    pub security_group_ids: Vec<String>,
    /// ARN of the instance profile given at launch.
    #[serde(default)]
    pub iam_instance_profile: Option<String>,
    #[serde(default)]
    pub client_token: Option<String>,
    #[serde(default = "default_root_device_name")]
    pub root_device_name: String,
    /// The root volume and every attached volume, in attach order.
    #[serde(default)]
    pub block_devices: Vec<BlockDevice>,
    /// Whether a libvirt virtual machine backs this instance.
    #[serde(default)]
    pub vm: bool,
}

/// One entry of an instance's block device mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDevice {
    pub device_name: String,
    pub volume_id: String,
    pub attach_time: String,
    pub delete_on_termination: bool,
}

/// An EBS volume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Volume {
    pub volume_id: String,
    pub size_gib: u64,
    pub volume_type: String,
    pub availability_zone: String,
    /// `available` | `in-use`.
    pub state: String,
    pub create_time: String,
    pub encrypted: bool,
    pub kms_key_id: Option<String>,
    pub iops: Option<i64>,
    pub throughput: Option<i64>,
    pub client_token: Option<String>,
    pub attachment: Option<VolumeAttachment>,
    pub tags: HashMap<String, String>,
}

/// Where a volume is attached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeAttachment {
    pub instance_id: String,
    pub device: String,
    pub attach_time: String,
    pub delete_on_termination: bool,
}

/// The availability zone an instance or volume reports when nothing
/// placed it anywhere more specific.
pub fn default_availability_zone() -> String {
    "us-east-1a".to_string()
}

fn default_root_device_name() -> String {
    "/dev/xvda".to_string()
}

impl Instance {
    /// Numeric state code per the EC2 wire format. Real SDKs key off `code`,
    /// not `name`, so it must stay in sync with `state`.
    pub fn state_code(&self) -> u32 {
        match self.state.as_str() {
            "pending" => 0,
            "running" => 16,
            "shutting-down" => 32,
            "terminated" => 48,
            "stopping" => 64,
            "stopped" => 80,
            _ => 16,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Address {
    pub allocation_id: String,
    pub public_ip: String,
    pub instance_id: Option<String>,
}
