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
    pub state: String,
    pub subnet_id: Option<String>,
    pub vpc_id: Option<String>,
    pub private_ip_address: Option<String>,
    pub launch_time: String,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Address {
    pub allocation_id: String,
    pub public_ip: String,
    pub instance_id: Option<String>,
}
