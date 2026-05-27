//! AWS Identity Store emulator. Users, groups, and group memberships scoped
//! by IdentityStoreId — usually paired with the existing `awsim-sso-admin`
//! service for full IAM Identity Center coverage.

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::pagination::{cap_max_results, decode_token, encode_token};
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::debug;

#[derive(Debug, Default)]
pub struct IdentityStoreState {
    /// (identity_store_id, user_id) keyed.
    pub users: DashMap<String, IdUser>,
    /// (identity_store_id, group_id) keyed.
    pub groups: DashMap<String, IdGroup>,
    /// (identity_store_id, membership_id) keyed.
    pub memberships: DashMap<String, GroupMembership>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdUser {
    pub identity_store_id: String,
    pub user_id: String,
    pub user_name: String,
    pub display_name: Option<String>,
    pub name: Option<Value>,
    pub emails: Vec<Value>,
    pub addresses: Vec<Value>,
    pub phone_numbers: Vec<Value>,
    pub user_type: Option<String>,
    pub title: Option<String>,
    pub preferred_language: Option<String>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdGroup {
    pub identity_store_id: String,
    pub group_id: String,
    pub display_name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMembership {
    pub identity_store_id: String,
    pub membership_id: String,
    pub group_id: String,
    pub member_user_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IdentityStoreSnapshot {
    pub users: Vec<IdUser>,
    pub groups: Vec<IdGroup>,
    pub memberships: Vec<GroupMembership>,
}

fn user_key(store: &str, user: &str) -> String {
    format!("{store}|{user}")
}
fn group_key(store: &str, group: &str) -> String {
    format!("{store}|{group}")
}
fn ms_key(store: &str, ms: &str) -> String {
    format!("{store}|{ms}")
}

impl IdentityStoreState {
    pub fn to_snapshot(&self) -> IdentityStoreSnapshot {
        IdentityStoreSnapshot {
            users: self.users.iter().map(|e| e.value().clone()).collect(),
            groups: self.groups.iter().map(|e| e.value().clone()).collect(),
            memberships: self.memberships.iter().map(|e| e.value().clone()).collect(),
        }
    }
    pub fn restore_from_snapshot(&self, snap: IdentityStoreSnapshot) {
        self.users.clear();
        self.groups.clear();
        self.memberships.clear();
        for u in snap.users {
            self.users
                .insert(user_key(&u.identity_store_id, &u.user_id), u);
        }
        for g in snap.groups {
            self.groups
                .insert(group_key(&g.identity_store_id, &g.group_id), g);
        }
        for m in snap.memberships {
            self.memberships
                .insert(ms_key(&m.identity_store_id, &m.membership_id), m);
        }
    }
}

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("ValidationException", format!("{key} is required")))
}

/// Build a `ResourceNotFoundException` carrying the AWS-documented
/// `ResourceType` and `ResourceId` extras. SDK clients branch on
/// these to attribute the miss to the right resource (USER / GROUP /
/// MEMBERSHIP) without parsing the message.
fn not_found_with_resource(
    resource_type: &'static str,
    resource_id: &str,
    msg: impl Into<String>,
) -> AwsError {
    AwsError::not_found("ResourceNotFoundException", msg)
        .with_extra("ResourceType", Value::String(resource_type.to_string()))
        .with_extra("ResourceId", Value::String(resource_id.to_string()))
}

/// IdentityStoreId regex per AWS: `^d-[0-9a-f]{10}$`. AWS rejects
/// other shapes (legacy `i-*` identifiers, garbage, etc.) at every
/// API boundary with `ValidationException`.
fn validate_identity_store_id(id: &str) -> Result<(), AwsError> {
    if id.len() != 12 {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "IdentityStoreId `{id}` must be 12 characters: `d-` + 10 lowercase hex digits."
            ),
        ));
    }
    let mut chars = id.chars();
    let (Some('d'), Some('-')) = (chars.next(), chars.next()) else {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("IdentityStoreId `{id}` must start with `d-`."),
        ));
    };
    if !chars.all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!("IdentityStoreId `{id}` must use lowercase hex after `d-`."),
        ));
    }
    Ok(())
}

/// AWS's documented bounds on `UserName`: required, 1..=128 chars,
/// no leading/trailing whitespace, no control characters.
fn validate_user_name(name: &str) -> Result<(), AwsError> {
    if name.is_empty() || name.len() > 128 {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "UserName must be 1..=128 characters; got {} chars.",
                name.chars().count()
            ),
        ));
    }
    if name.trim() != name {
        return Err(AwsError::bad_request(
            "ValidationException",
            "UserName must not have leading or trailing whitespace.",
        ));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(AwsError::bad_request(
            "ValidationException",
            "UserName must not contain control characters.",
        ));
    }
    Ok(())
}

/// Identity Store pagination cap: AWS's documented MaxResults default
/// and max for `ListUsers` / `ListGroups`.
const LIST_DEFAULT_MAX: usize = 100;

/// Encode a pagination cursor scoped to the calling identity store.
/// The token carries `<store>|<id>` so a stolen cursor cannot be
/// replayed against a different `IdentityStoreId`.
fn encode_tenant_token(store: &str, marker: &str) -> String {
    encode_token(&format!("{store}|{marker}"))
}

/// Decode a pagination cursor and refuse it when the embedded store
/// doesn't match the request's `IdentityStoreId`. Returns the
/// per-tenant marker on success.
fn decode_tenant_token(store: &str, token: &str) -> Result<String, AwsError> {
    let payload = decode_token(token)?;
    let (scope, marker) = payload
        .split_once('|')
        .ok_or_else(|| AwsError::bad_request("ValidationException", "NextToken is malformed."))?;
    if scope != store {
        return Err(AwsError::bad_request(
            "ValidationException",
            "NextToken does not belong to this IdentityStoreId.",
        ));
    }
    Ok(marker.to_string())
}

/// Group `DisplayName` cap: 1..=1024 chars per AWS docs. Optional on
/// User and required on Group; callers pass the relevant required
/// flag separately.
fn validate_display_name(name: &str) -> Result<(), AwsError> {
    if name.is_empty() || name.len() > 1024 {
        return Err(AwsError::bad_request(
            "ValidationException",
            format!(
                "DisplayName must be 1..=1024 characters; got {} chars.",
                name.chars().count()
            ),
        ));
    }
    Ok(())
}

/// Enforce SCIM-style validation on a multi-valued contact attribute
/// (`Emails`, `PhoneNumbers`, `Addresses`):
///   * at most one entry may carry `Primary: true`
///   * each entry's `Type` (if present) must be one of the documented
///     values; case-sensitive per the SCIM core schema.
fn validate_multivalued(
    field: &str,
    items: &[Value],
    allowed_types: &[&str],
) -> Result<(), AwsError> {
    let mut primary_seen = false;
    for entry in items {
        if entry
            .get("Primary")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            if primary_seen {
                return Err(AwsError::bad_request(
                    "ValidationException",
                    format!("`{field}` has more than one entry marked Primary."),
                ));
            }
            primary_seen = true;
        }
        if let Some(t) = entry.get("Type").and_then(|v| v.as_str())
            && !allowed_types.contains(&t)
        {
            return Err(AwsError::bad_request(
                "ValidationException",
                format!("`{field}` Type `{t}` is not in {allowed_types:?}."),
            ));
        }
    }
    Ok(())
}

fn user_to_value(u: &IdUser) -> Value {
    json!({
        "IdentityStoreId": u.identity_store_id,
        "UserId": u.user_id,
        "UserName": u.user_name,
        "DisplayName": u.display_name,
        "Name": u.name,
        "Emails": u.emails,
        "Addresses": u.addresses,
        "PhoneNumbers": u.phone_numbers,
        "UserType": u.user_type,
        "Title": u.title,
        "PreferredLanguage": u.preferred_language,
        "Locale": u.locale,
        "Timezone": u.timezone,
    })
}

fn group_to_value(g: &IdGroup) -> Value {
    json!({
        "IdentityStoreId": g.identity_store_id,
        "GroupId": g.group_id,
        "DisplayName": g.display_name,
        "Description": g.description,
    })
}

fn membership_to_value(m: &GroupMembership) -> Value {
    json!({
        "IdentityStoreId": m.identity_store_id,
        "MembershipId": m.membership_id,
        "GroupId": m.group_id,
        "MemberId": { "UserId": m.member_user_id },
    })
}

pub struct IdentityStoreService {
    store: AccountRegionStore<IdentityStoreState>,
}

impl IdentityStoreService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<IdentityStoreState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<IdentityStoreState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for IdentityStoreService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for IdentityStoreService {
    fn service_name(&self) -> &str {
        "identitystore"
    }

    fn signing_name(&self) -> &str {
        "identitystore"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "IdentityStore request");
        let state = self.get_state(ctx);
        match operation {
            "CreateUser" => {
                let store = require_str(&input, "IdentityStoreId")?.to_string();
                validate_identity_store_id(&store)?;
                let user_name = require_str(&input, "UserName")?.to_string();
                validate_user_name(&user_name)?;
                if let Some(d) = input.get("DisplayName").and_then(|v| v.as_str()) {
                    validate_display_name(d)?;
                }
                if let Some(arr) = input.get("Emails").and_then(|v| v.as_array()) {
                    validate_multivalued("Emails", arr, &["work", "home", "other"])?;
                }
                if let Some(arr) = input.get("PhoneNumbers").and_then(|v| v.as_array()) {
                    validate_multivalued(
                        "PhoneNumbers",
                        arr,
                        &["work", "home", "mobile", "fax", "pager", "other"],
                    )?;
                }
                if let Some(arr) = input.get("Addresses").and_then(|v| v.as_array()) {
                    validate_multivalued("Addresses", arr, &["work", "home", "other"])?;
                }
                let user_id = uuid::Uuid::new_v4().to_string();
                let u = IdUser {
                    identity_store_id: store.clone(),
                    user_id: user_id.clone(),
                    user_name,
                    display_name: input
                        .get("DisplayName")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    name: input.get("Name").cloned(),
                    emails: input
                        .get("Emails")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default(),
                    addresses: input
                        .get("Addresses")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default(),
                    phone_numbers: input
                        .get("PhoneNumbers")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default(),
                    user_type: input
                        .get("UserType")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    title: input
                        .get("Title")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    preferred_language: input
                        .get("PreferredLanguage")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    locale: input
                        .get("Locale")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    timezone: input
                        .get("Timezone")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                };
                state.users.insert(user_key(&store, &user_id), u);
                Ok(json!({ "UserId": user_id, "IdentityStoreId": store }))
            }
            "DescribeUser" => {
                let store = require_str(&input, "IdentityStoreId")?;
                let user_id = require_str(&input, "UserId")?;
                let u = state.users.get(&user_key(store, user_id)).ok_or_else(|| {
                    not_found_with_resource("USER", user_id, format!("User {user_id} not found"))
                })?;
                Ok(user_to_value(&u))
            }
            "GetUserId" => {
                let store = require_str(&input, "IdentityStoreId")?;
                let alt = input
                    .get("AlternateIdentifier")
                    .and_then(|a| a.get("UniqueAttribute"))
                    .and_then(|u| u.get("AttributeValue"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AwsError::bad_request(
                            "ValidationException",
                            "AlternateIdentifier.UniqueAttribute.AttributeValue is required",
                        )
                    })?;
                let user_id = state
                    .users
                    .iter()
                    .find(|e| e.value().identity_store_id == store && e.value().user_name == alt)
                    .map(|e| e.value().user_id.clone());
                let user_id = user_id.ok_or_else(|| {
                    not_found_with_resource("USER", alt, format!("User {alt} not found"))
                })?;
                Ok(json!({ "UserId": user_id, "IdentityStoreId": store }))
            }
            "ListUsers" => {
                let store = require_str(&input, "IdentityStoreId")?.to_string();
                validate_identity_store_id(&store)?;
                let max = cap_max_results(
                    input.get("MaxResults").and_then(|v| v.as_i64()),
                    LIST_DEFAULT_MAX,
                    LIST_DEFAULT_MAX,
                );
                let starting = match input.get("NextToken").and_then(|v| v.as_str()) {
                    Some(t) => Some(decode_tenant_token(&store, t)?),
                    None => None,
                };
                // Collect, sort by user_id (deterministic order so the
                // cursor resumes correctly across requests).
                let mut users: Vec<IdUser> = state
                    .users
                    .iter()
                    .filter(|e| e.value().identity_store_id == store)
                    .map(|e| e.value().clone())
                    .collect();
                users.sort_by(|a, b| a.user_id.cmp(&b.user_id));
                let start_idx = match starting {
                    Some(marker) => users
                        .iter()
                        .position(|u| u.user_id >= marker)
                        .unwrap_or(users.len()),
                    None => 0,
                };
                let take = max.min(users.len().saturating_sub(start_idx));
                let page_items: Vec<Value> = users[start_idx..start_idx + take]
                    .iter()
                    .map(user_to_value)
                    .collect();
                let next = if start_idx + take < users.len() {
                    Some(encode_tenant_token(
                        &store,
                        &users[start_idx + take].user_id,
                    ))
                } else {
                    None
                };
                let mut resp = json!({ "Users": page_items });
                if let Some(t) = next {
                    resp["NextToken"] = json!(t);
                }
                Ok(resp)
            }
            "UpdateUser" => {
                let store = require_str(&input, "IdentityStoreId")?;
                let user_id = require_str(&input, "UserId")?;
                let mut u = state
                    .users
                    .get_mut(&user_key(store, user_id))
                    .ok_or_else(|| {
                        not_found_with_resource(
                            "USER",
                            user_id,
                            format!("User {user_id} not found"),
                        )
                    })?;
                if let Some(ops) = input.get("Operations").and_then(|v| v.as_array()) {
                    for op in ops {
                        let path = op
                            .get("AttributePath")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let value = op.get("AttributeValue").cloned();
                        match path {
                            "displayName" => {
                                u.display_name = value.and_then(|v| v.as_str().map(String::from))
                            }
                            "title" => u.title = value.and_then(|v| v.as_str().map(String::from)),
                            "userType" => {
                                u.user_type = value.and_then(|v| v.as_str().map(String::from))
                            }
                            _ => {}
                        }
                    }
                }
                Ok(json!({}))
            }
            "DeleteUser" => {
                let store = require_str(&input, "IdentityStoreId")?.to_string();
                let user_id = require_str(&input, "UserId")?.to_string();
                state
                    .users
                    .remove(&user_key(&store, &user_id))
                    .ok_or_else(|| {
                        not_found_with_resource(
                            "USER",
                            &user_id,
                            format!("User {user_id} not found"),
                        )
                    })?;
                state
                    .memberships
                    .retain(|_, m| !(m.identity_store_id == store && m.member_user_id == user_id));
                Ok(json!({}))
            }
            "CreateGroup" => {
                let store = require_str(&input, "IdentityStoreId")?.to_string();
                validate_identity_store_id(&store)?;
                let display_name = require_str(&input, "DisplayName")?.to_string();
                validate_display_name(&display_name)?;
                let group_id = uuid::Uuid::new_v4().to_string();
                let g = IdGroup {
                    identity_store_id: store.clone(),
                    group_id: group_id.clone(),
                    display_name,
                    description: input
                        .get("Description")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                };
                state.groups.insert(group_key(&store, &group_id), g);
                Ok(json!({ "GroupId": group_id, "IdentityStoreId": store }))
            }
            "DescribeGroup" => {
                let store = require_str(&input, "IdentityStoreId")?;
                let group_id = require_str(&input, "GroupId")?;
                let g = state
                    .groups
                    .get(&group_key(store, group_id))
                    .ok_or_else(|| {
                        not_found_with_resource(
                            "GROUP",
                            group_id,
                            format!("Group {group_id} not found"),
                        )
                    })?;
                Ok(group_to_value(&g))
            }
            "ListGroups" => {
                let store = require_str(&input, "IdentityStoreId")?.to_string();
                validate_identity_store_id(&store)?;
                let max = cap_max_results(
                    input.get("MaxResults").and_then(|v| v.as_i64()),
                    LIST_DEFAULT_MAX,
                    LIST_DEFAULT_MAX,
                );
                let starting = match input.get("NextToken").and_then(|v| v.as_str()) {
                    Some(t) => Some(decode_tenant_token(&store, t)?),
                    None => None,
                };
                let mut groups: Vec<IdGroup> = state
                    .groups
                    .iter()
                    .filter(|e| e.value().identity_store_id == store)
                    .map(|e| e.value().clone())
                    .collect();
                groups.sort_by(|a, b| a.group_id.cmp(&b.group_id));
                let start_idx = match starting {
                    Some(marker) => groups
                        .iter()
                        .position(|g| g.group_id >= marker)
                        .unwrap_or(groups.len()),
                    None => 0,
                };
                let take = max.min(groups.len().saturating_sub(start_idx));
                let page_items: Vec<Value> = groups[start_idx..start_idx + take]
                    .iter()
                    .map(group_to_value)
                    .collect();
                let next = if start_idx + take < groups.len() {
                    Some(encode_tenant_token(
                        &store,
                        &groups[start_idx + take].group_id,
                    ))
                } else {
                    None
                };
                let mut resp = json!({ "Groups": page_items });
                if let Some(t) = next {
                    resp["NextToken"] = json!(t);
                }
                Ok(resp)
            }
            "UpdateGroup" => {
                let store = require_str(&input, "IdentityStoreId")?;
                let group_id = require_str(&input, "GroupId")?;
                let mut g = state
                    .groups
                    .get_mut(&group_key(store, group_id))
                    .ok_or_else(|| {
                        not_found_with_resource(
                            "GROUP",
                            group_id,
                            format!("Group {group_id} not found"),
                        )
                    })?;
                if let Some(ops) = input.get("Operations").and_then(|v| v.as_array()) {
                    for op in ops {
                        let path = op
                            .get("AttributePath")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let value = op.get("AttributeValue").cloned();
                        match path {
                            "displayName" => {
                                if let Some(s) = value.and_then(|v| v.as_str().map(String::from)) {
                                    g.display_name = s;
                                }
                            }
                            "description" => {
                                g.description = value.and_then(|v| v.as_str().map(String::from))
                            }
                            _ => {}
                        }
                    }
                }
                Ok(json!({}))
            }
            "DeleteGroup" => {
                let store = require_str(&input, "IdentityStoreId")?.to_string();
                let group_id = require_str(&input, "GroupId")?.to_string();
                state
                    .groups
                    .remove(&group_key(&store, &group_id))
                    .ok_or_else(|| {
                        not_found_with_resource(
                            "GROUP",
                            &group_id,
                            format!("Group {group_id} not found"),
                        )
                    })?;
                state
                    .memberships
                    .retain(|_, m| !(m.identity_store_id == store && m.group_id == group_id));
                Ok(json!({}))
            }
            "CreateGroupMembership" => {
                let store = require_str(&input, "IdentityStoreId")?.to_string();
                let group_id = require_str(&input, "GroupId")?.to_string();
                let member = input
                    .get("MemberId")
                    .and_then(|m| m.get("UserId"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AwsError::bad_request("ValidationException", "MemberId.UserId is required")
                    })?
                    .to_string();
                if !state.groups.contains_key(&group_key(&store, &group_id)) {
                    return Err(not_found_with_resource(
                        "GROUP",
                        &group_id,
                        format!("Group {group_id} not found"),
                    ));
                }
                let membership_id = uuid::Uuid::new_v4().to_string();
                let m = GroupMembership {
                    identity_store_id: store.clone(),
                    membership_id: membership_id.clone(),
                    group_id,
                    member_user_id: member,
                };
                state.memberships.insert(ms_key(&store, &membership_id), m);
                Ok(json!({ "MembershipId": membership_id, "IdentityStoreId": store }))
            }
            "DescribeGroupMembership" => {
                let store = require_str(&input, "IdentityStoreId")?;
                let membership_id = require_str(&input, "MembershipId")?;
                let m = state
                    .memberships
                    .get(&ms_key(store, membership_id))
                    .ok_or_else(|| {
                        not_found_with_resource(
                            "MEMBERSHIP",
                            membership_id,
                            format!("Membership {membership_id} not found"),
                        )
                    })?;
                Ok(membership_to_value(&m))
            }
            "ListGroupMemberships" => {
                let store = require_str(&input, "IdentityStoreId")?;
                let group_id = require_str(&input, "GroupId")?;
                let items: Vec<Value> = state
                    .memberships
                    .iter()
                    .filter(|e| {
                        let m = e.value();
                        m.identity_store_id == store && m.group_id == group_id
                    })
                    .map(|e| membership_to_value(e.value()))
                    .collect();
                Ok(json!({ "GroupMemberships": items }))
            }
            "IsMemberInGroups" => {
                // AWS-shape: `IsMemberInGroups(IdentityStoreId, MemberId,
                // GroupIds[])` -> `Results[{GroupId, MemberId,
                // MembershipExists}]`. Every GroupId must resolve to a
                // real group in the same store; a missing group is a
                // hard `ResourceNotFoundException` with
                // `ResourceType=GROUP` (not a soft `MembershipExists:false`).
                let store = require_str(&input, "IdentityStoreId")?.to_string();
                validate_identity_store_id(&store)?;
                let user_id = input
                    .get("MemberId")
                    .and_then(|m| m.get("UserId"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AwsError::bad_request("ValidationException", "MemberId.UserId is required")
                    })?
                    .to_string();
                let group_ids: Vec<String> = input
                    .get("GroupIds")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        AwsError::bad_request("ValidationException", "GroupIds is required")
                    })?
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                if group_ids.is_empty() {
                    return Err(AwsError::bad_request(
                        "ValidationException",
                        "GroupIds must contain at least one entry.",
                    ));
                }
                if group_ids.len() > 100 {
                    return Err(AwsError::bad_request(
                        "ValidationException",
                        format!(
                            "GroupIds has {} entries; the maximum is 100.",
                            group_ids.len()
                        ),
                    ));
                }
                let mut results = Vec::with_capacity(group_ids.len());
                for group_id in &group_ids {
                    if !state.groups.contains_key(&group_key(&store, group_id)) {
                        return Err(not_found_with_resource(
                            "GROUP",
                            group_id,
                            format!("Group `{group_id}` does not exist in store `{store}`."),
                        ));
                    }
                    let exists = state.memberships.iter().any(|e| {
                        let m = e.value();
                        m.identity_store_id == store
                            && m.group_id == *group_id
                            && m.member_user_id == user_id
                    });
                    results.push(json!({
                        "GroupId": group_id,
                        "MemberId": { "UserId": user_id.clone() },
                        "MembershipExists": exists,
                    }));
                }
                Ok(json!({ "Results": results }))
            }
            "ListGroupMembershipsForMember" => {
                let store = require_str(&input, "IdentityStoreId")?;
                let user_id = input
                    .get("MemberId")
                    .and_then(|m| m.get("UserId"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AwsError::bad_request("ValidationException", "MemberId.UserId is required")
                    })?;
                let items: Vec<Value> = state
                    .memberships
                    .iter()
                    .filter(|e| {
                        let m = e.value();
                        m.identity_store_id == store && m.member_user_id == user_id
                    })
                    .map(|e| membership_to_value(e.value()))
                    .collect();
                Ok(json!({ "GroupMemberships": items }))
            }
            "DeleteGroupMembership" => {
                let store = require_str(&input, "IdentityStoreId")?;
                let membership_id = require_str(&input, "MembershipId")?;
                state
                    .memberships
                    .remove(&ms_key(store, membership_id))
                    .ok_or_else(|| {
                        not_found_with_resource(
                            "MEMBERSHIP",
                            membership_id,
                            format!("Membership {membership_id} not found"),
                        )
                    })?;
                Ok(json!({}))
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = IdentityStoreSnapshot {
            users: vec![],
            groups: vec![],
            memberships: vec![],
        };
        for (_, st) in self.store.iter_all() {
            let s = st.to_snapshot();
            all.users.extend(s.users);
            all.groups.extend(s.groups);
            all.memberships.extend(s.memberships);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: IdentityStoreSnapshot =
            serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let st = self.store.get("000000000000", "us-east-1");
        st.restore_from_snapshot(snap);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("identitystore", "us-east-1")
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
            if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    #[test]
    fn user_group_membership_lifecycle() {
        let svc = IdentityStoreService::new();
        let ctx = ctx();
        let u = block_on(svc.handle(
            "CreateUser",
            json!({ "IdentityStoreId": "d-1234567890", "UserName": "alice@example.com" }),
            &ctx,
        ))
        .unwrap();
        let user_id = u["UserId"].as_str().unwrap().to_string();

        let g = block_on(svc.handle(
            "CreateGroup",
            json!({ "IdentityStoreId": "d-1234567890", "DisplayName": "engineers" }),
            &ctx,
        ))
        .unwrap();
        let group_id = g["GroupId"].as_str().unwrap().to_string();

        let ms = block_on(svc.handle(
            "CreateGroupMembership",
            json!({
                "IdentityStoreId": "d-1234567890",
                "GroupId": group_id,
                "MemberId": { "UserId": user_id }
            }),
            &ctx,
        ))
        .unwrap();
        assert!(ms["MembershipId"].as_str().is_some());

        let memberships = block_on(svc.handle(
            "ListGroupMembershipsForMember",
            json!({
                "IdentityStoreId": "d-1234567890",
                "MemberId": { "UserId": user_id }
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(memberships["GroupMemberships"].as_array().unwrap().len(), 1);

        // Deleting a user removes their memberships
        block_on(svc.handle(
            "DeleteUser",
            json!({ "IdentityStoreId": "d-1234567890", "UserId": user_id }),
            &ctx,
        ))
        .unwrap();
        let after = block_on(svc.handle(
            "ListGroupMemberships",
            json!({ "IdentityStoreId": "d-1234567890", "GroupId": group_id }),
            &ctx,
        ))
        .unwrap();
        assert!(after["GroupMemberships"].as_array().unwrap().is_empty());
    }

    #[test]
    fn get_user_id_by_username() {
        let svc = IdentityStoreService::new();
        let ctx = ctx();
        let u = block_on(svc.handle(
            "CreateUser",
            json!({ "IdentityStoreId": "d-0123456789", "UserName": "bob" }),
            &ctx,
        ))
        .unwrap();
        let r = block_on(svc.handle(
            "GetUserId",
            json!({
                "IdentityStoreId": "d-0123456789",
                "AlternateIdentifier": {
                    "UniqueAttribute": { "AttributePath": "userName", "AttributeValue": "bob" }
                }
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(r["UserId"], u["UserId"]);
    }

    #[test]
    fn create_user_rejects_malformed_identity_store_id() {
        let svc = IdentityStoreService::new();
        let ctx = RequestContext::new("identitystore", "us-east-1");
        for bad in ["d-12345", "x-1234567890", "d-123456789G", "garbage"] {
            let err = block_on(svc.handle(
                "CreateUser",
                json!({ "IdentityStoreId": bad, "UserName": "alice" }),
                &ctx,
            ))
            .unwrap_err();
            assert_eq!(err.code, "ValidationException", "input `{bad}`");
        }
    }

    #[test]
    fn create_user_rejects_oversized_user_name() {
        let svc = IdentityStoreService::new();
        let ctx = RequestContext::new("identitystore", "us-east-1");
        let long = "a".repeat(129);
        let err = block_on(svc.handle(
            "CreateUser",
            json!({ "IdentityStoreId": "d-0123456789", "UserName": long }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_user_rejects_whitespace_padded_user_name() {
        let svc = IdentityStoreService::new();
        let ctx = RequestContext::new("identitystore", "us-east-1");
        let err = block_on(svc.handle(
            "CreateUser",
            json!({ "IdentityStoreId": "d-0123456789", "UserName": " alice " }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("whitespace"), "{}", err.message);
    }

    #[test]
    fn not_found_errors_carry_resource_type_and_id() {
        let svc = IdentityStoreService::new();
        let ctx = RequestContext::new("identitystore", "us-east-1");
        let store = "d-0123456789";

        // USER scope
        let err = block_on(svc.handle(
            "DescribeUser",
            json!({ "IdentityStoreId": store, "UserId": "u-missing" }),
            &ctx,
        ))
        .unwrap_err();
        let extras = err.extras.as_ref().unwrap();
        assert_eq!(
            extras.get("ResourceType").and_then(|v| v.as_str()),
            Some("USER")
        );
        assert_eq!(
            extras.get("ResourceId").and_then(|v| v.as_str()),
            Some("u-missing")
        );

        // GROUP scope
        let err = block_on(svc.handle(
            "DescribeGroup",
            json!({ "IdentityStoreId": store, "GroupId": "g-missing" }),
            &ctx,
        ))
        .unwrap_err();
        let extras = err.extras.as_ref().unwrap();
        assert_eq!(
            extras.get("ResourceType").and_then(|v| v.as_str()),
            Some("GROUP")
        );
        assert_eq!(
            extras.get("ResourceId").and_then(|v| v.as_str()),
            Some("g-missing")
        );

        // MEMBERSHIP scope
        let err = block_on(svc.handle(
            "DescribeGroupMembership",
            json!({ "IdentityStoreId": store, "MembershipId": "m-missing" }),
            &ctx,
        ))
        .unwrap_err();
        let extras = err.extras.as_ref().unwrap();
        assert_eq!(
            extras.get("ResourceType").and_then(|v| v.as_str()),
            Some("MEMBERSHIP")
        );
        assert_eq!(
            extras.get("ResourceId").and_then(|v| v.as_str()),
            Some("m-missing")
        );
    }

    #[test]
    fn list_users_paginates_deterministically_and_scopes_token() {
        let svc = IdentityStoreService::new();
        let ctx = RequestContext::new("identitystore", "us-east-1");
        let store_a = "d-aaaaaaaaaa";
        let store_b = "d-bbbbbbbbbb";
        for i in 0..3 {
            block_on(svc.handle(
                "CreateUser",
                json!({ "IdentityStoreId": store_a, "UserName": format!("u{i}") }),
                &ctx,
            ))
            .unwrap();
        }
        // Page 1: receive a NextToken pointing past the first entry.
        let page1 = block_on(svc.handle(
            "ListUsers",
            json!({ "IdentityStoreId": store_a, "MaxResults": 1 }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(page1["Users"].as_array().unwrap().len(), 1);
        let token = page1["NextToken"]
            .as_str()
            .expect("first page must hand back a NextToken")
            .to_string();
        // Page 2 against the same tenant works.
        let page2 = block_on(svc.handle(
            "ListUsers",
            json!({ "IdentityStoreId": store_a, "MaxResults": 1, "NextToken": token.clone() }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(page2["Users"].as_array().unwrap().len(), 1);
        // Same token replayed against a *different* IdentityStoreId
        // must be rejected — that's the cross-tenant defence.
        let err = block_on(svc.handle(
            "ListUsers",
            json!({ "IdentityStoreId": store_b, "MaxResults": 1, "NextToken": token }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
        assert!(err.message.contains("IdentityStoreId"), "{}", err.message);
    }

    #[test]
    fn list_groups_paginates_with_stable_cursor() {
        let svc = IdentityStoreService::new();
        let ctx = RequestContext::new("identitystore", "us-east-1");
        let store = "d-aaaaaaaaaa";
        for i in 0..4 {
            block_on(svc.handle(
                "CreateGroup",
                json!({ "IdentityStoreId": store, "DisplayName": format!("g{i}") }),
                &ctx,
            ))
            .unwrap();
        }
        let page1 = block_on(svc.handle(
            "ListGroups",
            json!({ "IdentityStoreId": store, "MaxResults": 2 }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(page1["Groups"].as_array().unwrap().len(), 2);
        let token = page1["NextToken"].as_str().unwrap().to_string();
        let page2 = block_on(svc.handle(
            "ListGroups",
            json!({ "IdentityStoreId": store, "MaxResults": 2, "NextToken": token }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(page2["Groups"].as_array().unwrap().len(), 2);
        assert!(page2.get("NextToken").is_none());
    }

    #[test]
    fn is_member_in_groups_returns_per_group_existence() {
        let svc = IdentityStoreService::new();
        let ctx = RequestContext::new("identitystore", "us-east-1");
        let store = "d-0123456789";
        let u = block_on(svc.handle(
            "CreateUser",
            json!({ "IdentityStoreId": store, "UserName": "alice" }),
            &ctx,
        ))
        .unwrap();
        let g1 = block_on(svc.handle(
            "CreateGroup",
            json!({ "IdentityStoreId": store, "DisplayName": "engineers" }),
            &ctx,
        ))
        .unwrap();
        let g2 = block_on(svc.handle(
            "CreateGroup",
            json!({ "IdentityStoreId": store, "DisplayName": "ops" }),
            &ctx,
        ))
        .unwrap();
        // Add alice to engineers only.
        block_on(svc.handle(
            "CreateGroupMembership",
            json!({
                "IdentityStoreId": store,
                "GroupId": g1["GroupId"],
                "MemberId": { "UserId": u["UserId"] },
            }),
            &ctx,
        ))
        .unwrap();

        let resp = block_on(svc.handle(
            "IsMemberInGroups",
            json!({
                "IdentityStoreId": store,
                "MemberId": { "UserId": u["UserId"] },
                "GroupIds": [g1["GroupId"], g2["GroupId"]],
            }),
            &ctx,
        ))
        .unwrap();
        let results = resp["Results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        // The engineers entry should mark membership true; ops false.
        let engineers = results
            .iter()
            .find(|r| r["GroupId"] == g1["GroupId"])
            .unwrap();
        let ops = results
            .iter()
            .find(|r| r["GroupId"] == g2["GroupId"])
            .unwrap();
        assert_eq!(engineers["MembershipExists"], json!(true));
        assert_eq!(ops["MembershipExists"], json!(false));
    }

    #[test]
    fn is_member_in_groups_rejects_missing_group() {
        let svc = IdentityStoreService::new();
        let ctx = RequestContext::new("identitystore", "us-east-1");
        let store = "d-0123456789";
        let u = block_on(svc.handle(
            "CreateUser",
            json!({ "IdentityStoreId": store, "UserName": "alice" }),
            &ctx,
        ))
        .unwrap();
        let err = block_on(svc.handle(
            "IsMemberInGroups",
            json!({
                "IdentityStoreId": store,
                "MemberId": { "UserId": u["UserId"] },
                "GroupIds": ["g-does-not-exist"],
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ResourceNotFoundException");
        let extras = err.extras.as_ref().unwrap();
        assert_eq!(
            extras.get("ResourceType").and_then(|v| v.as_str()),
            Some("GROUP")
        );
    }

    #[test]
    fn is_member_in_groups_requires_non_empty_group_ids() {
        let svc = IdentityStoreService::new();
        let ctx = RequestContext::new("identitystore", "us-east-1");
        let err = block_on(svc.handle(
            "IsMemberInGroups",
            json!({
                "IdentityStoreId": "d-0123456789",
                "MemberId": { "UserId": "u-1" },
                "GroupIds": [],
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_group_rejects_oversized_display_name() {
        let svc = IdentityStoreService::new();
        let ctx = RequestContext::new("identitystore", "us-east-1");
        let long = "g".repeat(1025);
        let err = block_on(svc.handle(
            "CreateGroup",
            json!({ "IdentityStoreId": "d-0123456789", "DisplayName": long }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_user_rejects_two_primary_emails() {
        let svc = IdentityStoreService::new();
        let err = block_on(svc.handle(
            "CreateUser",
            json!({
                "IdentityStoreId": "d-1234567890",
                "UserName": "x",
                "Emails": [
                    { "Value": "a@x", "Primary": true, "Type": "work" },
                    { "Value": "b@x", "Primary": true, "Type": "home" },
                ],
            }),
            &ctx(),
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_user_rejects_unknown_email_type() {
        let svc = IdentityStoreService::new();
        let err = block_on(svc.handle(
            "CreateUser",
            json!({
                "IdentityStoreId": "d-1234567890",
                "UserName": "x",
                "Emails": [{ "Value": "a@x", "Type": "company" }],
            }),
            &ctx(),
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_user_rejects_unknown_phone_type() {
        let svc = IdentityStoreService::new();
        let err = block_on(svc.handle(
            "CreateUser",
            json!({
                "IdentityStoreId": "d-1234567890",
                "UserName": "x",
                "PhoneNumbers": [{ "Value": "+1", "Type": "satellite" }],
            }),
            &ctx(),
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_user_accepts_one_primary_per_attribute() {
        let svc = IdentityStoreService::new();
        block_on(svc.handle(
            "CreateUser",
            json!({
                "IdentityStoreId": "d-1234567890",
                "UserName": "x",
                "Emails": [
                    { "Value": "a@x", "Primary": true, "Type": "work" },
                    { "Value": "b@x", "Type": "home" },
                ],
                "PhoneNumbers": [
                    { "Value": "+1", "Type": "mobile" },
                ],
                "Addresses": [
                    { "Type": "home", "Primary": true },
                ],
            }),
            &ctx(),
        ))
        .unwrap();
    }
}
