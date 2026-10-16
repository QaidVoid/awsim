//! The `dynamodb:LeadingKeys` IAM condition key: the partition key values
//! a request touches, read from the request body and the key schema of
//! the table or index it targets.

use std::collections::HashMap;

use awsim_core::ContextValue;
use serde_json::Value;

use crate::operations;
use crate::state::{DynamoState, extract_scalar_str};

const KEY: &str = "dynamodb:LeadingKeys";

/// The partition key value of `item`, a `Key` or `Item` map addressed to
/// `table`. Number keys are compared as their string form.
pub(crate) fn of_item(state: &DynamoState, table: &str, item: &Value) -> Option<String> {
    let table = state.tables.get(table_name(table))?;
    extract_scalar_str(item.get(table.hash_key()?)?).map(str::to_string)
}

/// The partition key value a Query reads from its table or index.
pub(crate) fn of_query(state: &DynamoState, input: &Value) -> Option<String> {
    let table = state.tables.get(table_name(input["TableName"].as_str()?))?;
    operations::query::partition_key_value(&table, input)
}

/// The condition context for a check that touches `leading_key`. Empty
/// when the request carries no partition key value, so `ForAllValues`
/// conditions pass and `ForAnyValue` conditions fail, as in AWS.
pub(crate) fn context(leading_key: Option<String>) -> HashMap<String, ContextValue> {
    leading_key
        .map(|key| (KEY.to_string(), ContextValue::StringList(vec![key])))
        .into_iter()
        .collect()
}

/// The table name in a `TableName`, which DynamoDB also accepts as an ARN.
fn table_name(table: &str) -> &str {
    table
        .split_once(":table/")
        .map(|(_, rest)| rest.split('/').next().unwrap_or(rest))
        .unwrap_or(table)
}
