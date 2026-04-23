use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::SsoAdminState;

pub fn list_instances(
    state: &SsoAdminState,
    _input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    state.ensure_default_instance(&ctx.account_id, &ctx.region);

    let instances: Vec<Value> = state
        .instances
        .iter()
        .map(|e| {
            let i = e.value();
            json!({
                "InstanceArn": i.instance_arn,
                "IdentityStoreId": i.identity_store_id,
                "Name": i.name,
                "Status": i.status,
                "OwnerAccountId": ctx.account_id,
                "CreatedDate": 0,
            })
        })
        .collect();

    Ok(json!({ "Instances": instances }))
}
