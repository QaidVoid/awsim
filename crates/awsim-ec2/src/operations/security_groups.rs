use awsim_core::AwsError;
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    error::resource_not_found,
    ids::new_ec2_id,
    state::{self, Ec2State, IpPermission, IpRange, SecurityGroup},
};

use super::{opt_i64, opt_str, require_str};

pub fn sg_to_value(sg: &SecurityGroup) -> Value {
    json!({
        "groupId": sg.group_id,
        "groupName": sg.group_name,
        "groupDescription": sg.description,
        "vpcId": sg.vpc_id,
        "ipPermissions": { "item": ip_permissions_to_value(&sg.ip_permissions) },
        "ipPermissionsEgress": { "item": ip_permissions_to_value(&sg.ip_permissions_egress) },
    })
}

fn ip_permissions_to_value(perms: &[IpPermission]) -> Vec<Value> {
    perms
        .iter()
        .map(|p| {
            json!({
                "fromPort": p.from_port,
                "toPort": p.to_port,
                "ipProtocol": p.ip_protocol,
                "ipRanges": { "item": p.ip_ranges.iter().map(|r| json!({ "cidrIp": r.cidr_ip })).collect::<Vec<_>>() },
                "groups": { "item": p.user_id_group_pairs.iter().map(|g| {
                    let mut obj = json!({ "groupId": g.group_id });
                    if let Some(ref u) = g.user_id { obj["userId"] = json!(u); }
                    obj
                }).collect::<Vec<_>>() },
            })
        })
        .collect()
}

fn parse_ip_permissions(input: &Value) -> Vec<IpPermission> {
    let mut perms = Vec::new();

    // IpPermissions can come in as an array or as IpPermissions.member.N
    let items = match input.get("IpPermissions") {
        Some(Value::Array(arr)) => arr.clone(),
        Some(Value::Object(map)) => {
            // Handles member array from query parsing
            if let Some(Value::Array(arr)) = map.get("member") {
                arr.clone()
            } else {
                // Collect numbered keys
                let mut numbered: Vec<(usize, Value)> = map
                    .iter()
                    .filter_map(|(k, v)| k.parse::<usize>().ok().map(|n| (n, v.clone())))
                    .collect();
                numbered.sort_by_key(|(n, _)| *n);
                numbered.into_iter().map(|(_, v)| v).collect()
            }
        }
        _ => return perms,
    };

    for item in items {
        let from_port = opt_i64(&item, "FromPort");
        let to_port = opt_i64(&item, "ToPort");
        let ip_protocol = opt_str(&item, "IpProtocol").unwrap_or("-1").to_string();

        let ip_ranges = parse_ip_ranges(&item);
        let user_id_group_pairs = parse_user_id_group_pairs(&item);

        perms.push(IpPermission {
            from_port,
            to_port,
            ip_protocol,
            ip_ranges,
            user_id_group_pairs,
        });
    }

    perms
}

fn parse_user_id_group_pairs(input: &Value) -> Vec<state::UserIdGroupPair> {
    let mut out = Vec::new();
    let items = match input.get("UserIdGroupPairs") {
        Some(Value::Array(arr)) => arr.clone(),
        Some(Value::Object(map)) => {
            if let Some(Value::Array(arr)) = map.get("member") {
                arr.clone()
            } else {
                let mut numbered: Vec<(usize, Value)> = map
                    .iter()
                    .filter_map(|(k, v)| k.parse::<usize>().ok().map(|n| (n, v.clone())))
                    .collect();
                numbered.sort_by_key(|(n, _)| *n);
                numbered.into_iter().map(|(_, v)| v).collect()
            }
        }
        _ => return out,
    };
    for item in items {
        let group_id = match opt_str(&item, "GroupId") {
            Some(s) => s.to_string(),
            None => continue,
        };
        let user_id = opt_str(&item, "UserId").map(str::to_string);
        out.push(state::UserIdGroupPair { group_id, user_id });
    }
    out
}

fn parse_ip_ranges(input: &Value) -> Vec<IpRange> {
    let mut ranges = Vec::new();

    let items = match input.get("IpRanges") {
        Some(Value::Array(arr)) => arr.clone(),
        Some(Value::Object(map)) => {
            if let Some(Value::Array(arr)) = map.get("member") {
                arr.clone()
            } else {
                let mut numbered: Vec<(usize, Value)> = map
                    .iter()
                    .filter_map(|(k, v)| k.parse::<usize>().ok().map(|n| (n, v.clone())))
                    .collect();
                numbered.sort_by_key(|(n, _)| *n);
                numbered.into_iter().map(|(_, v)| v).collect()
            }
        }
        _ => return ranges,
    };

    for item in items {
        let cidr_ip = match opt_str(&item, "CidrIp") {
            Some(s) => s.to_string(),
            None => continue,
        };
        ranges.push(IpRange { cidr_ip });
    }

    ranges
}

pub fn create_security_group(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let group_name = require_str(input, "GroupName")?.to_string();
    let description = require_str(input, "Description")?.to_string();
    let vpc_id = require_str(input, "VpcId")?.to_string();

    if !state.vpcs.contains_key(&vpc_id) {
        return Err(resource_not_found("vpc", &vpc_id));
    }

    let group_id = new_ec2_id("sg");

    // Default egress: allow all
    let default_egress = IpPermission {
        from_port: None,
        to_port: None,
        ip_protocol: "-1".to_string(),
        ip_ranges: vec![IpRange {
            cidr_ip: "0.0.0.0/0".to_string(),
        }],
        user_id_group_pairs: Vec::new(),
    };

    let sg = SecurityGroup {
        group_id: group_id.clone(),
        group_name,
        description,
        vpc_id,
        ip_permissions: Vec::new(),
        ip_permissions_egress: vec![default_egress],
        tags: HashMap::new(),
    };

    state.security_groups.insert(group_id.clone(), sg);

    Ok(json!({ "groupId": group_id }))
}

pub fn delete_security_group(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let group_id = require_str(input, "GroupId")?;

    if state.security_groups.remove(group_id).is_none() {
        return Err(resource_not_found("security group", group_id));
    }

    Ok(json!({}))
}

pub fn describe_security_groups(state: &Ec2State, _input: &Value) -> Result<Value, AwsError> {
    let sgs: Vec<Value> = state
        .security_groups
        .iter()
        .map(|entry| sg_to_value(&entry))
        .collect();

    Ok(json!({ "securityGroupInfo": { "item": sgs } }))
}

pub fn authorize_security_group_ingress(
    state: &Ec2State,
    input: &Value,
) -> Result<Value, AwsError> {
    let group_id = require_str(input, "GroupId")?;
    let new_perms = parse_ip_permissions(input);

    let mut sg = state
        .security_groups
        .get_mut(group_id)
        .ok_or_else(|| resource_not_found("security group", group_id))?;

    sg.ip_permissions.extend(new_perms);

    Ok(json!({ "return": true }))
}

pub fn authorize_security_group_egress(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let group_id = require_str(input, "GroupId")?;
    let new_perms = parse_ip_permissions(input);

    let mut sg = state
        .security_groups
        .get_mut(group_id)
        .ok_or_else(|| resource_not_found("security group", group_id))?;

    sg.ip_permissions_egress.extend(new_perms);

    Ok(json!({ "return": true }))
}

pub fn revoke_security_group_ingress(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let group_id = require_str(input, "GroupId")?;
    let to_revoke = parse_ip_permissions(input);

    let mut sg = state
        .security_groups
        .get_mut(group_id)
        .ok_or_else(|| resource_not_found("security group", group_id))?;

    for perm in &to_revoke {
        sg.ip_permissions.retain(|p| {
            p.ip_protocol != perm.ip_protocol
                || p.from_port != perm.from_port
                || p.to_port != perm.to_port
        });
    }

    Ok(json!({ "return": true }))
}

pub fn revoke_security_group_egress(state: &Ec2State, input: &Value) -> Result<Value, AwsError> {
    let group_id = require_str(input, "GroupId")?;
    let to_revoke = parse_ip_permissions(input);

    let mut sg = state
        .security_groups
        .get_mut(group_id)
        .ok_or_else(|| resource_not_found("security group", group_id))?;

    for perm in &to_revoke {
        sg.ip_permissions_egress.retain(|p| {
            p.ip_protocol != perm.ip_protocol
                || p.from_port != perm.from_port
                || p.to_port != perm.to_port
        });
    }

    Ok(json!({ "return": true }))
}

#[cfg(test)]
mod user_id_group_pair_tests {
    use super::*;

    #[test]
    fn parses_user_id_group_pair_array() {
        let input = json!({
            "IpPermissions": [{
                "FromPort": 22,
                "ToPort": 22,
                "IpProtocol": "tcp",
                "UserIdGroupPairs": [
                    { "GroupId": "sg-abc" },
                    { "GroupId": "sg-def", "UserId": "111122223333" }
                ],
            }]
        });
        let perms = parse_ip_permissions(&input);
        assert_eq!(perms.len(), 1);
        let pairs = &perms[0].user_id_group_pairs;
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].group_id, "sg-abc");
        assert!(pairs[0].user_id.is_none());
        assert_eq!(pairs[1].group_id, "sg-def");
        assert_eq!(pairs[1].user_id.as_deref(), Some("111122223333"));
    }

    #[test]
    fn sg_to_value_surfaces_user_id_group_pairs() {
        let sg = SecurityGroup {
            group_id: "sg-aaa".to_string(),
            group_name: "n".to_string(),
            description: "d".to_string(),
            vpc_id: "vpc-1".to_string(),
            ip_permissions: vec![IpPermission {
                from_port: Some(80),
                to_port: Some(80),
                ip_protocol: "tcp".to_string(),
                ip_ranges: vec![],
                user_id_group_pairs: vec![state::UserIdGroupPair {
                    group_id: "sg-bbb".to_string(),
                    user_id: Some("000000000000".to_string()),
                }],
            }],
            ip_permissions_egress: vec![],
            tags: HashMap::new(),
        };
        let v = sg_to_value(&sg);
        let groups = &v["ipPermissions"]["item"][0]["groups"]["item"];
        assert_eq!(groups[0]["groupId"], "sg-bbb");
        assert_eq!(groups[0]["userId"], "000000000000");
    }
}
