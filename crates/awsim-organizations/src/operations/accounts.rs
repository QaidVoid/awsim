use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use crate::state::{Account, OrganizationsState, now_secs};

pub fn create_account(
    state: &OrganizationsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let email = input["Email"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "Email is required"))?;
    let name = input["AccountName"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "AccountName is required"))?;

    let account_id: String = format!(
        "{:012}",
        rand_num()
    );
    let arn = format!(
        "arn:aws:organizations::{}:account/{}/{}",
        ctx.account_id,
        state
            .organization
            .read()
            .unwrap()
            .as_ref()
            .map(|o| o.id.clone())
            .unwrap_or_else(|| "o-unknown".to_string()),
        account_id
    );

    let acc = Account {
        id: account_id.clone(),
        arn,
        email: email.to_string(),
        name: name.to_string(),
        status: "ACTIVE".to_string(),
        joined_method: "CREATED".to_string(),
        joined_timestamp: now_secs(),
    };
    state.accounts.insert(account_id.clone(), acc);

    let request_id = format!("car-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    Ok(json!({
        "CreateAccountStatus": {
            "Id": request_id,
            "AccountName": name,
            "State": "SUCCEEDED",
            "AccountId": account_id,
            "RequestedTimestamp": now_secs(),
        }
    }))
}

pub fn describe_account(
    state: &OrganizationsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = input["AccountId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("MissingParameter", "AccountId is required"))?;
    let acc = state
        .accounts
        .get(id)
        .ok_or_else(|| AwsError::not_found("AccountNotFoundException", format!("Account {id} not found")))?;
    Ok(json!({ "Account": serialize_account(&acc) }))
}

pub fn list_accounts(
    state: &OrganizationsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let accounts: Vec<Value> = state.accounts.iter().map(|e| serialize_account(e.value())).collect();
    Ok(json!({ "Accounts": accounts }))
}

pub fn list_accounts_for_parent(
    state: &OrganizationsState,
    _input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    list_accounts(state, &Value::Null, _ctx)
}

pub(crate) fn serialize_account(acc: &Account) -> Value {
    json!({
        "Id": acc.id,
        "Arn": acc.arn,
        "Email": acc.email,
        "Name": acc.name,
        "Status": acc.status,
        "JoinedMethod": acc.joined_method,
        "JoinedTimestamp": acc.joined_timestamp,
    })
}

fn rand_num() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    (nanos as u64) % 1_000_000_000_000
}
