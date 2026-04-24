use awsim_iam_policy::document::{
    ActionList, BaseOperator, Effect, Principal, ResourceList, SetQualifier,
};
use awsim_iam_policy::{ConditionOperator, parse};

#[test]
fn parse_admin_policy() {
    let json = r#"{
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Action": "*",
            "Resource": "*"
        }]
    }"#;
    let policy = parse(json).unwrap();
    assert_eq!(policy.version.as_deref(), Some("2012-10-17"));
    assert_eq!(policy.statements.len(), 1);
    let s = &policy.statements[0];
    assert_eq!(s.effect, Effect::Allow);
    assert!(matches!(s.action, Some(ActionList::Single(ref a)) if a == "*"));
    assert!(matches!(s.resource, Some(ResourceList::Single(ref r)) if r == "*"));
}

#[test]
fn parse_s3_readonly() {
    let json = r#"{
        "Version": "2012-10-17",
        "Statement": [{
            "Sid": "ReadOnly",
            "Effect": "Allow",
            "Action": ["s3:Get*", "s3:List*"],
            "Resource": ["arn:aws:s3:::example", "arn:aws:s3:::example/*"]
        }]
    }"#;
    let policy = parse(json).unwrap();
    let s = &policy.statements[0];
    assert_eq!(s.sid.as_deref(), Some("ReadOnly"));
    let actions: Vec<_> = s.action.as_ref().unwrap().iter().collect();
    assert_eq!(actions, vec!["s3:Get*", "s3:List*"]);
    let resources: Vec<_> = s.resource.as_ref().unwrap().iter().collect();
    assert_eq!(resources.len(), 2);
}

#[test]
fn parse_single_statement_object() {
    let json = r#"{
        "Statement": {
            "Effect": "Allow",
            "Action": "ec2:*",
            "Resource": "*"
        }
    }"#;
    let policy = parse(json).unwrap();
    assert_eq!(policy.statements.len(), 1);
}

#[test]
fn parse_principal_wildcard() {
    let json = r#"{
        "Statement": [{
            "Effect": "Allow",
            "Principal": "*",
            "Action": "s3:GetObject",
            "Resource": "arn:aws:s3:::pub/*"
        }]
    }"#;
    let policy = parse(json).unwrap();
    assert!(matches!(
        policy.statements[0].principal,
        Some(Principal::Wildcard)
    ));
}

#[test]
fn parse_principal_aws_string() {
    let json = r#"{
        "Statement": [{
            "Effect": "Allow",
            "Principal": {"AWS": "arn:aws:iam::123:role/Foo"},
            "Action": "*",
            "Resource": "*"
        }]
    }"#;
    let policy = parse(json).unwrap();
    let p = policy.statements[0].principal.as_ref().unwrap();
    assert!(matches!(p, Principal::Aws(v) if v.len() == 1));
}

#[test]
fn parse_principal_aws_list() {
    let json = r#"{
        "Statement": [{
            "Effect": "Allow",
            "Principal": {"AWS": ["arn:aws:iam::123:role/A","arn:aws:iam::456:role/B"]},
            "Action": "*",
            "Resource": "*"
        }]
    }"#;
    let policy = parse(json).unwrap();
    let p = policy.statements[0].principal.as_ref().unwrap();
    assert!(matches!(p, Principal::Aws(v) if v.len() == 2));
}

#[test]
fn parse_conditions_block() {
    let json = r#"{
        "Statement": [{
            "Effect": "Allow",
            "Action": "*",
            "Resource": "*",
            "Condition": {
                "StringEquals": {"aws:username": "alice"},
                "IpAddress": {"aws:SourceIp": ["10.0.0.0/8","192.168.1.0/24"]}
            }
        }]
    }"#;
    let policy = parse(json).unwrap();
    let cb = policy.statements[0].condition.as_ref().unwrap();
    assert_eq!(cb.conditions.len(), 2);
}

#[test]
fn parse_invalid_effect() {
    let json = r#"{"Statement":[{"Effect":"Permit","Action":"*","Resource":"*"}]}"#;
    assert!(parse(json).is_err());
}

#[test]
fn parse_unknown_version() {
    let json = r#"{"Version":"2099-01-01","Statement":[]}"#;
    assert!(parse(json).is_err());
}

#[test]
fn parse_not_action_not_resource() {
    let json = r#"{
        "Statement":[{
            "Effect": "Deny",
            "NotAction": "s3:GetObject",
            "NotResource": "arn:aws:s3:::safe/*"
        }]
    }"#;
    let policy = parse(json).unwrap();
    let s = &policy.statements[0];
    assert!(s.not_action.is_some());
    assert!(s.not_resource.is_some());
}

#[test]
fn parse_condition_operator_qualifiers() {
    let op = ConditionOperator::parse("ForAllValues:StringEqualsIfExists").unwrap();
    assert_eq!(op.base, BaseOperator::StringEquals);
    assert!(op.if_exists);
    assert_eq!(op.set_qualifier, SetQualifier::ForAllValues);

    let op = ConditionOperator::parse("ForAnyValue:ArnLike").unwrap();
    assert_eq!(op.base, BaseOperator::ArnLike);
    assert_eq!(op.set_qualifier, SetQualifier::ForAnyValue);
    assert!(!op.if_exists);

    assert!(ConditionOperator::parse("Bogus").is_err());
}

#[test]
fn parse_condition_numeric_value() {
    let json = r#"{
        "Statement":[{
            "Effect":"Allow","Action":"*","Resource":"*",
            "Condition": {"NumericLessThan": {"s3:max-keys": 100}}
        }]
    }"#;
    let policy = parse(json).unwrap();
    let cond = &policy.statements[0].condition.as_ref().unwrap().conditions[0];
    assert_eq!(cond.values, vec!["100"]);
}

#[test]
fn parse_condition_bool_value() {
    let json = r#"{
        "Statement":[{
            "Effect":"Deny","Action":"*","Resource":"*",
            "Condition": {"Bool": {"aws:SecureTransport": false}}
        }]
    }"#;
    let policy = parse(json).unwrap();
    let cond = &policy.statements[0].condition.as_ref().unwrap().conditions[0];
    assert_eq!(cond.values, vec!["false"]);
}
