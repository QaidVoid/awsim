use crate::error::ParseError;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct PolicyDocument {
    pub version: Option<String>,
    pub id: Option<String>,
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub sid: Option<String>,
    pub effect: Effect,
    pub principal: Option<Principal>,
    pub not_principal: Option<Principal>,
    pub action: Option<ActionList>,
    pub not_action: Option<ActionList>,
    pub resource: Option<ResourceList>,
    pub not_resource: Option<ResourceList>,
    pub condition: Option<ConditionBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Principal {
    Wildcard,
    Aws(Vec<String>),
    Service(Vec<String>),
    Federated(Vec<String>),
    CanonicalUser(Vec<String>),
    Mixed {
        aws: Vec<String>,
        service: Vec<String>,
        federated: Vec<String>,
        canonical_user: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionList {
    Single(String),
    Multiple(Vec<String>),
}

impl ActionList {
    pub fn iter(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        match self {
            ActionList::Single(s) => Box::new(std::iter::once(s.as_str())),
            ActionList::Multiple(v) => Box::new(v.iter().map(|s| s.as_str())),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResourceList {
    Single(String),
    Multiple(Vec<String>),
}

impl ResourceList {
    pub fn iter(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        match self {
            ResourceList::Single(s) => Box::new(std::iter::once(s.as_str())),
            ResourceList::Multiple(v) => Box::new(v.iter().map(|s| s.as_str())),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConditionBlock {
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub operator: ConditionOperator,
    pub key: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetQualifier {
    None,
    ForAllValues,
    ForAnyValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseOperator {
    StringEquals,
    StringNotEquals,
    StringEqualsIgnoreCase,
    StringNotEqualsIgnoreCase,
    StringLike,
    StringNotLike,
    NumericEquals,
    NumericNotEquals,
    NumericLessThan,
    NumericLessThanEquals,
    NumericGreaterThan,
    NumericGreaterThanEquals,
    DateEquals,
    DateNotEquals,
    DateLessThan,
    DateLessThanEquals,
    DateGreaterThan,
    DateGreaterThanEquals,
    Bool,
    BinaryEquals,
    IpAddress,
    NotIpAddress,
    ArnEquals,
    ArnLike,
    ArnNotEquals,
    ArnNotLike,
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConditionOperator {
    pub base: BaseOperator,
    pub if_exists: bool,
    pub set_qualifier: SetQualifier,
}

impl ConditionOperator {
    pub fn parse(raw: &str) -> Result<Self, ParseError> {
        let mut s = raw;
        let mut set_qualifier = SetQualifier::None;
        if let Some(rest) = s.strip_prefix("ForAllValues:") {
            set_qualifier = SetQualifier::ForAllValues;
            s = rest;
        } else if let Some(rest) = s.strip_prefix("ForAnyValue:") {
            set_qualifier = SetQualifier::ForAnyValue;
            s = rest;
        }
        let mut if_exists = false;
        if let Some(rest) = s.strip_suffix("IfExists") {
            if_exists = true;
            s = rest;
        }
        let base = match s {
            "StringEquals" => BaseOperator::StringEquals,
            "StringNotEquals" => BaseOperator::StringNotEquals,
            "StringEqualsIgnoreCase" => BaseOperator::StringEqualsIgnoreCase,
            "StringNotEqualsIgnoreCase" => BaseOperator::StringNotEqualsIgnoreCase,
            "StringLike" => BaseOperator::StringLike,
            "StringNotLike" => BaseOperator::StringNotLike,
            "NumericEquals" => BaseOperator::NumericEquals,
            "NumericNotEquals" => BaseOperator::NumericNotEquals,
            "NumericLessThan" => BaseOperator::NumericLessThan,
            "NumericLessThanEquals" => BaseOperator::NumericLessThanEquals,
            "NumericGreaterThan" => BaseOperator::NumericGreaterThan,
            "NumericGreaterThanEquals" => BaseOperator::NumericGreaterThanEquals,
            "DateEquals" => BaseOperator::DateEquals,
            "DateNotEquals" => BaseOperator::DateNotEquals,
            "DateLessThan" => BaseOperator::DateLessThan,
            "DateLessThanEquals" => BaseOperator::DateLessThanEquals,
            "DateGreaterThan" => BaseOperator::DateGreaterThan,
            "DateGreaterThanEquals" => BaseOperator::DateGreaterThanEquals,
            "Bool" => BaseOperator::Bool,
            "BinaryEquals" => BaseOperator::BinaryEquals,
            "IpAddress" => BaseOperator::IpAddress,
            "NotIpAddress" => BaseOperator::NotIpAddress,
            "ArnEquals" => BaseOperator::ArnEquals,
            "ArnLike" => BaseOperator::ArnLike,
            "ArnNotEquals" => BaseOperator::ArnNotEquals,
            "ArnNotLike" => BaseOperator::ArnNotLike,
            "Null" => BaseOperator::Null,
            other => return Err(ParseError::InvalidConditionOperator(other.to_string())),
        };
        Ok(ConditionOperator {
            base,
            if_exists,
            set_qualifier,
        })
    }
}

pub fn parse(json: &str) -> Result<PolicyDocument, ParseError> {
    let value: Value = serde_json::from_str(json)?;
    parse_value(&value)
}

pub fn parse_value(v: &Value) -> Result<PolicyDocument, ParseError> {
    let obj = v
        .as_object()
        .ok_or_else(|| ParseError::InvalidShape("policy must be object".into()))?;
    let version = obj
        .get("Version")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    if let Some(ref ver) = version
        && ver != "2012-10-17"
        && ver != "2008-10-17"
    {
        return Err(ParseError::UnknownVersion(ver.clone()));
    }
    let id = obj
        .get("Id")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let stmt_v = obj
        .get("Statement")
        .ok_or_else(|| ParseError::MissingField("Statement".into()))?;
    let statements = match stmt_v {
        Value::Array(arr) => arr
            .iter()
            .map(parse_statement)
            .collect::<Result<Vec<_>, _>>()?,
        Value::Object(_) => vec![parse_statement(stmt_v)?],
        _ => return Err(ParseError::InvalidShape("Statement".into())),
    };
    Ok(PolicyDocument {
        version,
        id,
        statements,
    })
}

fn parse_statement(v: &Value) -> Result<Statement, ParseError> {
    let obj = v
        .as_object()
        .ok_or_else(|| ParseError::InvalidShape("Statement entry must be object".into()))?;
    let sid = obj
        .get("Sid")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let effect_str = obj
        .get("Effect")
        .and_then(|x| x.as_str())
        .ok_or_else(|| ParseError::MissingField("Effect".into()))?;
    let effect = match effect_str {
        "Allow" => Effect::Allow,
        "Deny" => Effect::Deny,
        other => return Err(ParseError::InvalidEffect(other.to_string())),
    };
    let principal = obj.get("Principal").map(parse_principal).transpose()?;
    let not_principal = obj.get("NotPrincipal").map(parse_principal).transpose()?;
    let action = obj.get("Action").map(parse_action).transpose()?;
    let not_action = obj.get("NotAction").map(parse_action).transpose()?;
    let resource = obj.get("Resource").map(parse_resource).transpose()?;
    let not_resource = obj.get("NotResource").map(parse_resource).transpose()?;
    let condition = obj.get("Condition").map(parse_condition_block).transpose()?;
    Ok(Statement {
        sid,
        effect,
        principal,
        not_principal,
        action,
        not_action,
        resource,
        not_resource,
        condition,
    })
}

fn string_or_string_list(v: &Value) -> Result<Vec<String>, ParseError> {
    match v {
        Value::String(s) => Ok(vec![s.clone()]),
        Value::Array(arr) => arr
            .iter()
            .map(|x| {
                x.as_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| ParseError::InvalidShape("expected string".into()))
            })
            .collect(),
        _ => Err(ParseError::InvalidShape("expected string or list".into())),
    }
}

fn parse_action(v: &Value) -> Result<ActionList, ParseError> {
    match v {
        Value::String(s) => Ok(ActionList::Single(s.clone())),
        Value::Array(_) => Ok(ActionList::Multiple(string_or_string_list(v)?)),
        _ => Err(ParseError::InvalidShape("Action".into())),
    }
}

fn parse_resource(v: &Value) -> Result<ResourceList, ParseError> {
    match v {
        Value::String(s) => Ok(ResourceList::Single(s.clone())),
        Value::Array(_) => Ok(ResourceList::Multiple(string_or_string_list(v)?)),
        _ => Err(ParseError::InvalidShape("Resource".into())),
    }
}

fn parse_principal(v: &Value) -> Result<Principal, ParseError> {
    match v {
        Value::String(s) if s == "*" => Ok(Principal::Wildcard),
        Value::String(_) => Err(ParseError::InvalidShape(
            "Principal string must be \"*\"".into(),
        )),
        Value::Object(map) => {
            let mut aws = Vec::new();
            let mut service = Vec::new();
            let mut federated = Vec::new();
            let mut canonical_user = Vec::new();
            let mut count = 0;
            for (k, val) in map {
                match k.as_str() {
                    "AWS" => {
                        aws = string_or_string_list(val)?;
                        count += 1;
                    }
                    "Service" => {
                        service = string_or_string_list(val)?;
                        count += 1;
                    }
                    "Federated" => {
                        federated = string_or_string_list(val)?;
                        count += 1;
                    }
                    "CanonicalUser" => {
                        canonical_user = string_or_string_list(val)?;
                        count += 1;
                    }
                    other => {
                        return Err(ParseError::InvalidShape(format!(
                            "unknown principal type: {other}"
                        )));
                    }
                }
            }
            if count == 1 {
                if !aws.is_empty() {
                    return Ok(Principal::Aws(aws));
                }
                if !service.is_empty() {
                    return Ok(Principal::Service(service));
                }
                if !federated.is_empty() {
                    return Ok(Principal::Federated(federated));
                }
                if !canonical_user.is_empty() {
                    return Ok(Principal::CanonicalUser(canonical_user));
                }
            }
            Ok(Principal::Mixed {
                aws,
                service,
                federated,
                canonical_user,
            })
        }
        _ => Err(ParseError::InvalidShape("Principal".into())),
    }
}

fn parse_condition_block(v: &Value) -> Result<ConditionBlock, ParseError> {
    let map = v
        .as_object()
        .ok_or_else(|| ParseError::InvalidShape("Condition must be object".into()))?;
    let mut conditions = Vec::new();
    let sorted: BTreeMap<&String, &Value> = map.iter().collect();
    for (op_str, kv) in sorted {
        let operator = ConditionOperator::parse(op_str)?;
        let kv_map = kv
            .as_object()
            .ok_or_else(|| ParseError::InvalidShape("Condition operator value".into()))?;
        let sorted_keys: BTreeMap<&String, &Value> = kv_map.iter().collect();
        for (key, val) in sorted_keys {
            let values = condition_values(val)?;
            conditions.push(Condition {
                operator,
                key: key.clone(),
                values,
            });
        }
    }
    Ok(ConditionBlock { conditions })
}

fn condition_values(v: &Value) -> Result<Vec<String>, ParseError> {
    match v {
        Value::String(s) => Ok(vec![s.clone()]),
        Value::Bool(b) => Ok(vec![b.to_string()]),
        Value::Number(n) => Ok(vec![n.to_string()]),
        Value::Array(arr) => arr
            .iter()
            .map(|x| match x {
                Value::String(s) => Ok(s.clone()),
                Value::Bool(b) => Ok(b.to_string()),
                Value::Number(n) => Ok(n.to_string()),
                _ => Err(ParseError::InvalidShape("condition value".into())),
            })
            .collect(),
        _ => Err(ParseError::InvalidShape("condition value".into())),
    }
}

impl<'de> Deserialize<'de> for PolicyDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        parse_value(&value).map_err(serde::de::Error::custom)
    }
}
