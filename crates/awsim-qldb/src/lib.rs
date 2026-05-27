//! Amazon QLDB emulator. Stores ledger metadata only — the journal/ION query
//! data plane is not implemented.

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
pub struct QldbState {
    pub ledgers: DashMap<String, Ledger>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ledger {
    pub name: String,
    pub arn: String,
    pub state: String,
    pub creation_date_time: f64,
    pub permissions_mode: String,
    pub deletion_protection: bool,
    pub kms_key_arn: Option<String>,
    pub tags: HashMap<String, String>,
    /// `EncryptionStatus` field of the documented
    /// `EncryptionDescription` block. Persisted on the model so a
    /// future tick driver can flip it to `KMS_KEY_INACCESSIBLE` /
    /// `UPDATING` without rebuilding the structure on every read.
    #[serde(default = "default_encryption_status")]
    pub encryption_status: String,
    /// Epoch seconds when the KMS key first became inaccessible.
    /// `None` while the key is reachable; surfaced as JSON null on
    /// the API response.
    #[serde(default)]
    pub inaccessible_kms_key_date_time: Option<f64>,
}

fn default_encryption_status() -> String {
    "ENABLED".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QldbSnapshot {
    pub ledgers: Vec<Ledger>,
}

impl QldbState {
    pub fn to_snapshot(&self) -> QldbSnapshot {
        QldbSnapshot {
            ledgers: self.ledgers.iter().map(|e| e.value().clone()).collect(),
        }
    }
    pub fn restore_from_snapshot(&self, snap: QldbSnapshot) {
        self.ledgers.clear();
        for l in snap.ledgers {
            self.ledgers.insert(l.name.clone(), l);
        }
    }
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", format!("{key} is required")))
}

fn ledger_arn(ctx: &RequestContext, name: &str) -> String {
    format!(
        "arn:aws:qldb:{}:{}:ledger/{}",
        ctx.region, ctx.account_id, name
    )
}

fn ledger_to_value(l: &Ledger) -> Value {
    // `EncryptionDescription` documents three fields. The emulator
    // never simulates KMS key inaccessibility on its own, but the
    // status and inaccessible-date are persisted on the model so a
    // future tick driver can mutate them without changing the wire
    // shape.
    let inaccessible = match l.inaccessible_kms_key_date_time {
        Some(t) => json!(t),
        None => Value::Null,
    };
    json!({
        "Name": l.name,
        "Arn": l.arn,
        "State": l.state,
        "CreationDateTime": l.creation_date_time,
        "PermissionsMode": l.permissions_mode,
        "DeletionProtection": l.deletion_protection,
        "KmsKeyArn": l.kms_key_arn,
        "EncryptionDescription": {
            "KmsKeyArn": l.kms_key_arn,
            "EncryptionStatus": l.encryption_status,
            "InaccessibleKmsKeyDateTime": inaccessible,
        },
    })
}

pub struct QldbService {
    store: AccountRegionStore<QldbState>,
}

impl QldbService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<QldbState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<QldbState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for QldbService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for QldbService {
    fn service_name(&self) -> &str {
        "qldb"
    }

    fn signing_name(&self) -> &str {
        "qldb"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "POST",
                path_pattern: "/ledgers",
                operation: "CreateLedger",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/ledgers",
                operation: "ListLedgers",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/ledgers/{name}",
                operation: "DescribeLedger",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PATCH",
                path_pattern: "/ledgers/{name}",
                operation: "UpdateLedger",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/ledgers/{name}",
                operation: "DeleteLedger",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PATCH",
                path_pattern: "/ledgers/{name}/permissions-mode",
                operation: "UpdateLedgerPermissionsMode",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/tags/{resourceArn}",
                operation: "TagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/tags/{resourceArn}",
                operation: "UntagResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/tags/{resourceArn}",
                operation: "ListTagsForResource",
                required_query_param: None,
            },
        ]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "QLDB request");
        let state = self.get_state(ctx);
        match operation {
            "CreateLedger" => {
                let name = require_str(&input, "Name")?.to_string();
                if state.ledgers.contains_key(&name) {
                    return Err(AwsError::conflict(
                        "ResourceAlreadyExistsException",
                        format!("Ledger {name} already exists"),
                    ));
                }
                let tags: HashMap<String, String> = input
                    .get("Tags")
                    .and_then(|v| v.as_object())
                    .map(|o| {
                        o.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect()
                    })
                    .unwrap_or_default();
                let permissions_mode = require_str(&input, "PermissionsMode")?.to_string();
                if !matches!(permissions_mode.as_str(), "ALLOW_ALL" | "STANDARD") {
                    return Err(AwsError::bad_request(
                        "ValidationException",
                        format!(
                            "PermissionsMode `{permissions_mode}` must be ALLOW_ALL or STANDARD.",
                        ),
                    ));
                }
                let l = Ledger {
                    name: name.clone(),
                    arn: ledger_arn(ctx, &name),
                    state: "ACTIVE".to_string(),
                    creation_date_time: now(),
                    permissions_mode,
                    deletion_protection: input
                        .get("DeletionProtection")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    kms_key_arn: input
                        .get("KmsKey")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    tags,
                    encryption_status: default_encryption_status(),
                    inaccessible_kms_key_date_time: None,
                };
                let result = ledger_to_value(&l);
                state.ledgers.insert(name, l);
                Ok(result)
            }
            "DescribeLedger" => {
                let name = require_str(&input, "name").or_else(|_| require_str(&input, "Name"))?;
                let l = state.ledgers.get(name).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Ledger {name} not found"),
                    )
                })?;
                Ok(ledger_to_value(&l))
            }
            "ListLedgers" => {
                let items: Vec<Value> = state
                    .ledgers
                    .iter()
                    .map(|e| {
                        let l = e.value();
                        json!({ "Name": l.name, "State": l.state, "CreationDateTime": l.creation_date_time })
                    })
                    .collect();
                Ok(json!({ "Ledgers": items }))
            }
            "UpdateLedger" => {
                let name = require_str(&input, "name").or_else(|_| require_str(&input, "Name"))?;
                let mut l = state.ledgers.get_mut(name).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Ledger {name} not found"),
                    )
                })?;
                if let Some(d) = input.get("DeletionProtection").and_then(|v| v.as_bool()) {
                    l.deletion_protection = d;
                }
                if let Some(k) = input.get("KmsKey").and_then(|v| v.as_str()) {
                    l.kms_key_arn = Some(k.to_string());
                }
                Ok(ledger_to_value(&l))
            }
            "UpdateLedgerPermissionsMode" => {
                let name = require_str(&input, "name").or_else(|_| require_str(&input, "Name"))?;
                let mode = require_str(&input, "PermissionsMode")?.to_string();
                if !matches!(mode.as_str(), "ALLOW_ALL" | "STANDARD") {
                    return Err(AwsError::bad_request(
                        "ValidationException",
                        format!("PermissionsMode `{mode}` must be ALLOW_ALL or STANDARD."),
                    ));
                }
                let mut l = state.ledgers.get_mut(name).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Ledger {name} not found"),
                    )
                })?;
                l.permissions_mode = mode;
                Ok(json!({
                    "Name": l.name,
                    "Arn": l.arn,
                    "PermissionsMode": l.permissions_mode,
                }))
            }
            "DeleteLedger" => {
                let name = require_str(&input, "name").or_else(|_| require_str(&input, "Name"))?;
                let l = state.ledgers.get(name).ok_or_else(|| {
                    AwsError::not_found(
                        "ResourceNotFoundException",
                        format!("Ledger {name} not found"),
                    )
                })?;
                if l.deletion_protection {
                    return Err(AwsError::bad_request(
                        "ResourcePreconditionNotMetException",
                        "Disable DeletionProtection before deleting the ledger",
                    ));
                }
                drop(l);
                state.ledgers.remove(name);
                Ok(json!({}))
            }
            "TagResource" => {
                let arn = input
                    .get("resourceArn")
                    .or_else(|| input.get("ResourceArn"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let name = arn.rsplit('/').next().unwrap_or("");
                if let Some(mut l) = state.ledgers.get_mut(name)
                    && let Some(tags) = input
                        .get("Tags")
                        .or_else(|| input.get("tags"))
                        .and_then(|v| v.as_object())
                {
                    for (k, v) in tags {
                        if let Some(s) = v.as_str() {
                            l.tags.insert(k.clone(), s.to_string());
                        }
                    }
                }
                Ok(json!({}))
            }
            "UntagResource" => {
                let arn = input
                    .get("resourceArn")
                    .or_else(|| input.get("ResourceArn"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let name = arn.rsplit('/').next().unwrap_or("");
                if let Some(mut l) = state.ledgers.get_mut(name)
                    && let Some(keys) = input
                        .get("TagKeys")
                        .or_else(|| input.get("tagKeys"))
                        .and_then(|v| v.as_array())
                {
                    for k in keys {
                        if let Some(s) = k.as_str() {
                            l.tags.remove(s);
                        }
                    }
                }
                Ok(json!({}))
            }
            "ListTagsForResource" => {
                let arn = input
                    .get("resourceArn")
                    .or_else(|| input.get("ResourceArn"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let name = arn.rsplit('/').next().unwrap_or("");
                let tags = state
                    .ledgers
                    .get(name)
                    .map(|l| l.tags.clone())
                    .unwrap_or_default();
                Ok(json!({ "Tags": tags }))
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = QldbSnapshot { ledgers: vec![] };
        for (_, st) in self.store.iter_all() {
            all.ledgers.extend(st.to_snapshot().ledgers);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: QldbSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let st = self.store.get("000000000000", "us-east-1");
        st.restore_from_snapshot(snap);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("qldb", "us-east-1")
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
    fn deletion_protection_blocks_delete() {
        let svc = QldbService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateLedger",
            json!({ "Name": "audit", "PermissionsMode": "STANDARD" }),
            &ctx,
        ))
        .unwrap();
        let err =
            block_on(svc.handle("DeleteLedger", json!({ "name": "audit" }), &ctx)).unwrap_err();
        assert_eq!(err.code, "ResourcePreconditionNotMetException");
        block_on(svc.handle(
            "UpdateLedger",
            json!({ "name": "audit", "DeletionProtection": false }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle("DeleteLedger", json!({ "name": "audit" }), &ctx)).unwrap();
    }

    #[test]
    fn update_ledger_accepts_kms_key_and_surfaces_encryption_description() {
        let svc = QldbService::new();
        let ctx = RequestContext::new("qldb", "us-east-1");
        block_on(svc.handle(
            "CreateLedger",
            json!({ "Name": "kms-led", "PermissionsMode": "STANDARD", "DeletionProtection": false }),
            &ctx,
        ))
        .unwrap();

        let kms_key = "arn:aws:kms:us-east-1:123456789012:key/abcdef01-2345-6789-abcd-ef0123456789";
        let resp = block_on(svc.handle(
            "UpdateLedger",
            json!({ "name": "kms-led", "KmsKey": kms_key }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(resp["KmsKeyArn"], kms_key);
        let enc = &resp["EncryptionDescription"];
        assert_eq!(enc["KmsKeyArn"], kms_key);
        assert_eq!(enc["EncryptionStatus"], "ENABLED");
        assert!(
            enc.get("InaccessibleKmsKeyDateTime")
                .map(|v| v.is_null())
                .unwrap_or(false),
            "expected InaccessibleKmsKeyDateTime to be present as null, got {enc:?}",
        );
    }

    #[test]
    fn update_ledger_permissions_mode_persists() {
        let svc = QldbService::new();
        let ctx = RequestContext::new("qldb", "us-east-1");
        block_on(svc.handle(
            "CreateLedger",
            json!({
                "Name": "audit-mode",
                "PermissionsMode": "ALLOW_ALL",
                "DeletionProtection": false,
            }),
            &ctx,
        ))
        .unwrap();

        let resp = block_on(svc.handle(
            "UpdateLedgerPermissionsMode",
            json!({ "name": "audit-mode", "PermissionsMode": "STANDARD" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(resp["PermissionsMode"], "STANDARD");

        let described =
            block_on(svc.handle("DescribeLedger", json!({ "name": "audit-mode" }), &ctx)).unwrap();
        assert_eq!(described["PermissionsMode"], "STANDARD");

        // Round trip via UpdateLedgerPermissionsMode for the other variant.
        let resp = block_on(svc.handle(
            "UpdateLedgerPermissionsMode",
            json!({ "name": "audit-mode", "PermissionsMode": "ALLOW_ALL" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(resp["PermissionsMode"], "ALLOW_ALL");

        // Bad value still rejected.
        let err = block_on(svc.handle(
            "UpdateLedgerPermissionsMode",
            json!({ "name": "audit-mode", "PermissionsMode": "ROOT" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_ledger_rejects_unknown_permissions_mode() {
        let svc = QldbService::new();
        let ctx = RequestContext::new("qldb", "us-east-1");
        let err = block_on(svc.handle(
            "CreateLedger",
            json!({ "Name": "x", "PermissionsMode": "WIDE_OPEN" }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "ValidationException");
    }

    #[test]
    fn create_ledger_accepts_documented_permissions_modes() {
        let svc = QldbService::new();
        let ctx = RequestContext::new("qldb", "us-east-1");
        for mode in ["ALLOW_ALL", "STANDARD"] {
            block_on(svc.handle(
                "CreateLedger",
                json!({
                    "Name": format!("ledger-{mode}"),
                    "PermissionsMode": mode,
                    "DeletionProtection": false,
                }),
                &ctx,
            ))
            .unwrap();
        }
    }
}
