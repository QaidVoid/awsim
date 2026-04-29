use dashmap::DashMap;
use std::collections::HashMap;

/// EC2 state — per account+region.
#[derive(Debug, Default)]
pub struct Ec2State {
    pub vpcs: DashMap<String, Vpc>,
    pub subnets: DashMap<String, Subnet>,
    pub security_groups: DashMap<String, SecurityGroup>,
    pub internet_gateways: DashMap<String, InternetGateway>,
    pub route_tables: DashMap<String, RouteTable>,
    pub key_pairs: DashMap<String, KeyPair>,
    /// instanceId → Instance
    pub instances: DashMap<String, Instance>,
    /// Elastic IP allocation id → Address
    pub addresses: DashMap<String, Address>,
    /// resource-specific tags: resource_id → (key → value)
    pub resource_tags: DashMap<String, HashMap<String, String>>,
    /// Per-subnet host-octet cursor for the next launched instance. Real EC2
    /// allocates from the subnet's CIDR; we just bump a counter starting
    /// at host .10 (real EC2 reserves the first 4 addresses anyway).
    pub subnet_next_host: DashMap<String, u32>,
}

#[derive(Debug, Clone)]
pub struct Vpc {
    pub vpc_id: String,
    pub cidr_block: String,
    pub state: String,
    pub is_default: bool,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Subnet {
    pub subnet_id: String,
    pub vpc_id: String,
    pub cidr_block: String,
    pub availability_zone: String,
    pub state: String,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct SecurityGroup {
    pub group_id: String,
    pub group_name: String,
    pub description: String,
    pub vpc_id: String,
    pub ip_permissions: Vec<IpPermission>,
    pub ip_permissions_egress: Vec<IpPermission>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct IpPermission {
    pub from_port: Option<i64>,
    pub to_port: Option<i64>,
    pub ip_protocol: String,
    pub ip_ranges: Vec<IpRange>,
}

#[derive(Debug, Clone)]
pub struct IpRange {
    pub cidr_ip: String,
}

#[derive(Debug, Clone)]
pub struct InternetGateway {
    pub internet_gateway_id: String,
    /// VPC ID if attached, None if detached
    pub attached_vpc_id: Option<String>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct RouteTable {
    pub route_table_id: String,
    pub vpc_id: String,
    pub routes: Vec<Route>,
    /// subnet_id -> association_id
    pub associations: HashMap<String, String>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Route {
    pub destination_cidr_block: String,
    pub gateway_id: Option<String>,
    pub state: String,
}

#[derive(Debug, Clone)]
pub struct KeyPair {
    pub key_name: String,
    pub key_fingerprint: String,
    pub create_time: String,
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub instance_id: String,
    pub instance_type: String,
    pub image_id: String,
    /// EC2 lifecycle state — `pending` | `running` | `stopping` | `stopped`
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
    /// All instances from a single RunInstances batch share a reservation —
    /// DescribeInstances groups them under one reservationSet entry.
    pub reservation_id: String,
    pub tags: HashMap<String, String>,
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

#[derive(Debug, Clone)]
pub struct Address {
    pub allocation_id: String,
    pub public_ip: String,
    pub instance_id: Option<String>,
}
