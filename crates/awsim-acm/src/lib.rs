mod operations;
mod state;

pub use state::AcmState;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{AccountRegionStore, AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;
use tracing::debug;

use state::{AcmAccountConfig, AcmStateSnapshot};

/// The ACM (Certificate Manager) service handler.
pub struct AcmService {
    store: AccountRegionStore<AcmState>,
}

impl AcmService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<AcmState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for AcmService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for AcmService {
    fn service_name(&self) -> &str {
        "acm"
    }

    fn signing_name(&self) -> &str {
        "acm"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_1
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "ACM request");
        let state = self.get_state(ctx);

        match operation {
            "RequestCertificate" => {
                operations::certificates::request_certificate(&state, &input, ctx)
            }
            "DescribeCertificate" => {
                operations::certificates::describe_certificate(&state, &input, ctx)
            }
            "ListCertificates" => operations::certificates::list_certificates(&state, &input, ctx),
            "DeleteCertificate" => {
                operations::certificates::delete_certificate(&state, &input, ctx)
            }
            "GetCertificate" => operations::certificates::get_certificate(&state, &input, ctx),
            "ExportCertificate" => {
                operations::certificates::export_certificate(&state, &input, ctx)
            }
            "ImportCertificate" => {
                operations::certificates::import_certificate(&state, &input, ctx)
            }
            "RenewCertificate" => operations::certificates::renew_certificate(&state, &input, ctx),
            "UpdateCertificateOptions" => {
                operations::certificates::update_certificate_options(&state, &input, ctx)
            }
            "ResendValidationEmail" => {
                operations::certificates::resend_validation_email(&state, &input, ctx)
            }
            "AddTagsToCertificate" => {
                operations::tags::add_tags_to_certificate(&state, &input, ctx)
            }
            "RemoveTagsFromCertificate" => {
                operations::tags::remove_tags_from_certificate(&state, &input, ctx)
            }
            "ListTagsForCertificate" => {
                operations::tags::list_tags_for_certificate(&state, &input, ctx)
            }
            "PutAccountConfiguration" => {
                let expiry_config = input.get("ExpiryEvents").cloned();
                let config = AcmAccountConfig {
                    expiry_events_configuration: expiry_config,
                };
                state.account_config.insert("default".to_string(), config);
                Ok(serde_json::json!({}))
            }
            "GetAccountConfiguration" => {
                let config = state
                    .account_config
                    .get("default")
                    .map(|c| {
                        serde_json::json!({
                            "ExpiryEvents": c.expiry_events_configuration,
                        })
                    })
                    .unwrap_or_else(|| serde_json::json!({ "ExpiryEvents": null }));
                Ok(config)
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let certs: Vec<_> = self
            .store
            .iter_all()
            .into_iter()
            .flat_map(|(_, state)| state.to_snapshot().certificates)
            .collect();

        serde_json::to_vec(&AcmStateSnapshot {
            certificates: certs,
        })
        .ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snapshot: AcmStateSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;

        use std::collections::HashMap;
        let mut by_acct_region: HashMap<(String, String), Vec<state::Certificate>> = HashMap::new();

        // ARN: arn:aws:acm:{region}:{account}:certificate/{id}
        for cert in snapshot.certificates {
            let parts: Vec<&str> = cert.certificate_arn.splitn(6, ':').collect();
            let (account, region) = if parts.len() >= 5 {
                (parts[4].to_string(), parts[3].to_string())
            } else {
                ("000000000000".to_string(), "us-east-1".to_string())
            };
            by_acct_region
                .entry((account, region))
                .or_default()
                .push(cert);
        }

        for ((account, region), certs) in by_acct_region {
            let state = self.store.get(&account, &region);
            state.restore_from_snapshot(AcmStateSnapshot {
                certificates: certs,
            });
        }

        Ok(())
    }
}
