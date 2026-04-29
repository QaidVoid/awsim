//! AWS Transfer Family emulator. Servers, users, and SSH public keys.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::debug;

#[derive(Debug, Default)]
pub struct TransferState {
    pub servers: DashMap<String, Server>,
    /// (server_id, username) keyed.
    pub users: DashMap<String, TransferUser>,
    /// (server_id, username, key_id) keyed.
    pub ssh_keys: DashMap<String, SshKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub server_id: String,
    pub arn: String,
    pub state: String,
    pub protocols: Vec<String>,
    pub identity_provider_type: String,
    pub endpoint_type: String,
    pub domain: String,
    pub user_count: u32,
    pub logging_role: Option<String>,
    pub created: f64,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferUser {
    pub server_id: String,
    pub user_name: String,
    pub arn: String,
    pub home_directory: Option<String>,
    pub home_directory_type: String,
    pub policy: Option<String>,
    pub role: String,
    pub posix_profile: Option<Value>,
    pub ssh_public_key_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshKey {
    pub server_id: String,
    pub user_name: String,
    pub ssh_public_key_id: String,
    pub ssh_public_key_body: String,
    pub date_imported: f64,
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input.get(key).and_then(|v| v.as_str()).ok_or_else(|| {
        AwsError::bad_request("InvalidRequestException", format!("{key} is required"))
    })
}

fn user_key(server_id: &str, user_name: &str) -> String {
    format!("{server_id}|{user_name}")
}

fn server_arn(ctx: &RequestContext, id: &str) -> String {
    format!(
        "arn:aws:transfer:{}:{}:server/{}",
        ctx.region, ctx.account_id, id
    )
}

fn user_arn(ctx: &RequestContext, server_id: &str, user_name: &str) -> String {
    format!(
        "arn:aws:transfer:{}:{}:user/{}/{}",
        ctx.region, ctx.account_id, server_id, user_name
    )
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransferSnapshot {
    pub servers: Vec<Server>,
    pub users: Vec<TransferUser>,
    pub ssh_keys: Vec<SshKey>,
}

impl TransferState {
    pub fn to_snapshot(&self) -> TransferSnapshot {
        TransferSnapshot {
            servers: self.servers.iter().map(|e| e.value().clone()).collect(),
            users: self.users.iter().map(|e| e.value().clone()).collect(),
            ssh_keys: self.ssh_keys.iter().map(|e| e.value().clone()).collect(),
        }
    }
    pub fn restore_from_snapshot(&self, snap: TransferSnapshot) {
        self.servers.clear();
        self.users.clear();
        self.ssh_keys.clear();
        for s in snap.servers {
            self.servers.insert(s.server_id.clone(), s);
        }
        for u in snap.users {
            self.users.insert(user_key(&u.server_id, &u.user_name), u);
        }
        for k in snap.ssh_keys {
            let key = format!("{}|{}|{}", k.server_id, k.user_name, k.ssh_public_key_id);
            self.ssh_keys.insert(key, k);
        }
    }
}

fn server_to_value(s: &Server) -> Value {
    json!({
        "ServerId": s.server_id,
        "Arn": s.arn,
        "State": s.state,
        "Protocols": s.protocols,
        "IdentityProviderType": s.identity_provider_type,
        "EndpointType": s.endpoint_type,
        "Domain": s.domain,
        "UserCount": s.user_count,
        "LoggingRole": s.logging_role,
        "CreatedDate": s.created,
        "Tags": s.tags,
    })
}

fn user_to_value(u: &TransferUser) -> Value {
    json!({
        "ServerId": u.server_id,
        "UserName": u.user_name,
        "Arn": u.arn,
        "HomeDirectory": u.home_directory,
        "HomeDirectoryType": u.home_directory_type,
        "Policy": u.policy,
        "Role": u.role,
        "PosixProfile": u.posix_profile,
        "SshPublicKeyCount": u.ssh_public_key_count,
    })
}

pub struct TransferService {
    store: AccountRegionStore<TransferState>,
}

impl TransferService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<TransferState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<TransferState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for TransferService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for TransferService {
    fn service_name(&self) -> &str {
        "transfer"
    }

    fn signing_name(&self) -> &str {
        "transfer"
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
        debug!(operation, "Transfer request");
        let state = self.get_state(ctx);
        match operation {
            "CreateServer" => {
                let id = format!("s-{}", &uuid::Uuid::new_v4().simple().to_string()[..17]);
                let protocols = input
                    .get("Protocols")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_else(|| vec!["SFTP".to_string()]);
                let tags = input
                    .get("Tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|t| {
                                Some((
                                    t.get("Key")?.as_str()?.to_string(),
                                    t.get("Value")?.as_str()?.to_string(),
                                ))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let s = Server {
                    server_id: id.clone(),
                    arn: server_arn(ctx, &id),
                    state: "ONLINE".to_string(),
                    protocols,
                    identity_provider_type: input
                        .get("IdentityProviderType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("SERVICE_MANAGED")
                        .to_string(),
                    endpoint_type: input
                        .get("EndpointType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("PUBLIC")
                        .to_string(),
                    domain: input
                        .get("Domain")
                        .and_then(|v| v.as_str())
                        .unwrap_or("S3")
                        .to_string(),
                    user_count: 0,
                    logging_role: input
                        .get("LoggingRole")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    created: now(),
                    tags,
                };
                state.servers.insert(id.clone(), s);
                Ok(json!({ "ServerId": id }))
            }
            "DescribeServer" => {
                let id = require_str(&input, "ServerId")?;
                let s = state.servers.get(id).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Server {id} not found"),
                    )
                })?;
                Ok(json!({ "Server": server_to_value(&s) }))
            }
            "ListServers" => {
                let items: Vec<Value> = state
                    .servers
                    .iter()
                    .map(|e| {
                        let s = e.value();
                        json!({
                            "Arn": s.arn,
                            "ServerId": s.server_id,
                            "State": s.state,
                            "IdentityProviderType": s.identity_provider_type,
                            "EndpointType": s.endpoint_type,
                            "UserCount": s.user_count,
                        })
                    })
                    .collect();
                Ok(json!({ "Servers": items }))
            }
            "DeleteServer" => {
                let id = require_str(&input, "ServerId")?.to_string();
                state.servers.remove(&id).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Server {id} not found"),
                    )
                })?;
                let prefix = format!("{id}|");
                state.users.retain(|k, _| !k.starts_with(&prefix));
                state.ssh_keys.retain(|k, _| !k.starts_with(&prefix));
                Ok(json!({}))
            }
            "StartServer" => {
                let id = require_str(&input, "ServerId")?;
                let mut s = state.servers.get_mut(id).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Server {id} not found"),
                    )
                })?;
                s.state = "ONLINE".to_string();
                Ok(json!({}))
            }
            "StopServer" => {
                let id = require_str(&input, "ServerId")?;
                let mut s = state.servers.get_mut(id).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Server {id} not found"),
                    )
                })?;
                s.state = "OFFLINE".to_string();
                Ok(json!({}))
            }
            "CreateUser" => {
                let server_id = require_str(&input, "ServerId")?.to_string();
                let user_name = require_str(&input, "UserName")?.to_string();
                if !state.servers.contains_key(&server_id) {
                    return Err(AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Server {server_id} not found"),
                    ));
                }
                let role = require_str(&input, "Role")?.to_string();
                let key = user_key(&server_id, &user_name);
                if state.users.contains_key(&key) {
                    return Err(AwsError::conflict(
                        "ResourceExistsException",
                        format!("User {user_name} already exists"),
                    ));
                }
                let u = TransferUser {
                    server_id: server_id.clone(),
                    user_name: user_name.clone(),
                    arn: user_arn(ctx, &server_id, &user_name),
                    home_directory: input
                        .get("HomeDirectory")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    home_directory_type: input
                        .get("HomeDirectoryType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("PATH")
                        .to_string(),
                    policy: input
                        .get("Policy")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    role,
                    posix_profile: input.get("PosixProfile").cloned(),
                    ssh_public_key_count: 0,
                };
                state.users.insert(key, u);
                if let Some(mut s) = state.servers.get_mut(&server_id) {
                    s.user_count += 1;
                }
                Ok(json!({ "ServerId": server_id, "UserName": user_name }))
            }
            "DescribeUser" => {
                let server_id = require_str(&input, "ServerId")?;
                let user_name = require_str(&input, "UserName")?;
                let u = state
                    .users
                    .get(&user_key(server_id, user_name))
                    .ok_or_else(|| {
                        AwsError::not_found(
                            "ResourceNotFoundException",
                            format!("User {user_name} not found"),
                        )
                    })?;
                Ok(json!({ "ServerId": server_id, "User": user_to_value(&u) }))
            }
            "ListUsers" => {
                let server_id = require_str(&input, "ServerId")?;
                let items: Vec<Value> = state
                    .users
                    .iter()
                    .filter(|e| e.value().server_id == server_id)
                    .map(|e| {
                        let u = e.value();
                        json!({
                            "Arn": u.arn,
                            "UserName": u.user_name,
                            "HomeDirectory": u.home_directory,
                            "HomeDirectoryType": u.home_directory_type,
                            "Role": u.role,
                            "SshPublicKeyCount": u.ssh_public_key_count,
                        })
                    })
                    .collect();
                Ok(json!({ "ServerId": server_id, "Users": items }))
            }
            "DeleteUser" => {
                let server_id = require_str(&input, "ServerId")?.to_string();
                let user_name = require_str(&input, "UserName")?.to_string();
                state
                    .users
                    .remove(&user_key(&server_id, &user_name))
                    .ok_or_else(|| {
                        AwsError::not_found(
                            "ResourceNotFoundException",
                            format!("User {user_name} not found"),
                        )
                    })?;
                if let Some(mut s) = state.servers.get_mut(&server_id)
                    && s.user_count > 0
                {
                    s.user_count -= 1;
                }
                let prefix = format!("{server_id}|{user_name}|");
                state.ssh_keys.retain(|k, _| !k.starts_with(&prefix));
                Ok(json!({}))
            }
            "UpdateUser" => {
                let server_id = require_str(&input, "ServerId")?;
                let user_name = require_str(&input, "UserName")?;
                let mut u = state
                    .users
                    .get_mut(&user_key(server_id, user_name))
                    .ok_or_else(|| {
                        AwsError::not_found(
                            "ResourceNotFoundException",
                            format!("User {user_name} not found"),
                        )
                    })?;
                if let Some(p) = input.get("Policy").and_then(|v| v.as_str()) {
                    u.policy = Some(p.to_string());
                }
                if let Some(h) = input.get("HomeDirectory").and_then(|v| v.as_str()) {
                    u.home_directory = Some(h.to_string());
                }
                if let Some(r) = input.get("Role").and_then(|v| v.as_str()) {
                    u.role = r.to_string();
                }
                Ok(json!({ "ServerId": server_id, "UserName": user_name }))
            }
            "ImportSshPublicKey" => {
                let server_id = require_str(&input, "ServerId")?.to_string();
                let user_name = require_str(&input, "UserName")?.to_string();
                let body = require_str(&input, "SshPublicKeyBody")?.to_string();
                let key_id = format!("key-{}", &uuid::Uuid::new_v4().simple().to_string()[..16]);
                let key_full = format!("{server_id}|{user_name}|{key_id}");
                let k = SshKey {
                    server_id: server_id.clone(),
                    user_name: user_name.clone(),
                    ssh_public_key_id: key_id.clone(),
                    ssh_public_key_body: body,
                    date_imported: now(),
                };
                state.ssh_keys.insert(key_full, k);
                if let Some(mut u) = state.users.get_mut(&user_key(&server_id, &user_name)) {
                    u.ssh_public_key_count += 1;
                }
                Ok(json!({
                    "ServerId": server_id,
                    "UserName": user_name,
                    "SshPublicKeyId": key_id,
                }))
            }
            "DeleteSshPublicKey" => {
                let server_id = require_str(&input, "ServerId")?.to_string();
                let user_name = require_str(&input, "UserName")?.to_string();
                let key_id = require_str(&input, "SshPublicKeyId")?.to_string();
                let full = format!("{server_id}|{user_name}|{key_id}");
                state.ssh_keys.remove(&full).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Key {key_id} not found"),
                    )
                })?;
                if let Some(mut u) = state.users.get_mut(&user_key(&server_id, &user_name))
                    && u.ssh_public_key_count > 0
                {
                    u.ssh_public_key_count -= 1;
                }
                Ok(json!({}))
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = TransferSnapshot {
            servers: vec![],
            users: vec![],
            ssh_keys: vec![],
        };
        for (_, st) in self.store.iter_all() {
            let s = st.to_snapshot();
            all.servers.extend(s.servers);
            all.users.extend(s.users);
            all.ssh_keys.extend(s.ssh_keys);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: TransferSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let st = self.store.get("000000000000", "us-east-1");
        st.restore_from_snapshot(snap);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("transfer", "us-east-1")
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
    fn server_user_ssh_lifecycle() {
        let svc = TransferService::new();
        let ctx = ctx();
        let s = block_on(svc.handle("CreateServer", json!({}), &ctx)).unwrap();
        let server_id = s["ServerId"].as_str().unwrap().to_string();
        block_on(svc.handle(
            "CreateUser",
            json!({
                "ServerId": server_id,
                "UserName": "alice",
                "Role": "arn:aws:iam::000000000000:role/SftpRole"
            }),
            &ctx,
        ))
        .unwrap();
        let key = block_on(svc.handle(
            "ImportSshPublicKey",
            json!({
                "ServerId": server_id,
                "UserName": "alice",
                "SshPublicKeyBody": "ssh-rsa AAAA..."
            }),
            &ctx,
        ))
        .unwrap();
        let key_id = key["SshPublicKeyId"].as_str().unwrap();

        let users =
            block_on(svc.handle("ListUsers", json!({ "ServerId": server_id }), &ctx)).unwrap();
        assert_eq!(users["Users"].as_array().unwrap().len(), 1);
        assert_eq!(users["Users"][0]["SshPublicKeyCount"], 1);

        block_on(svc.handle(
            "DeleteSshPublicKey",
            json!({ "ServerId": server_id, "UserName": "alice", "SshPublicKeyId": key_id }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "DeleteUser",
            json!({ "ServerId": server_id, "UserName": "alice" }),
            &ctx,
        ))
        .unwrap();
        let described =
            block_on(svc.handle("DescribeServer", json!({ "ServerId": server_id }), &ctx)).unwrap();
        assert_eq!(described["Server"]["UserCount"], 0);
    }
}
