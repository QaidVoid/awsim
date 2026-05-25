use std::time::{SystemTime, UNIX_EPOCH};

use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::{SsmDocument, SsmState};

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn build_document_arn(ctx: &RequestContext, name: &str) -> String {
    format!(
        "arn:aws:ssm:{}:{}:document/{}",
        ctx.region, ctx.account_id, name
    )
}

fn document_description(doc: &SsmDocument) -> Value {
    json!({
        "Name": doc.name,
        "DocumentVersion": doc.document_version,
        "Status": doc.status,
        "DocumentType": doc.document_type,
        "DocumentFormat": doc.document_format,
        "CreatedDate": doc.created_date,
    })
}

// ---------------------------------------------------------------------------
// CreateDocument
// ---------------------------------------------------------------------------

pub fn create_document(
    state: &SsmState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?
        .to_string();

    let content = input["Content"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Content is required"))?
        .to_string();

    if state.documents.contains_key(&name) {
        return Err(AwsError::bad_request(
            "DocumentAlreadyExists",
            format!("Document '{name}' already exists"),
        ));
    }

    let document_type = input["DocumentType"]
        .as_str()
        .unwrap_or("Command")
        .to_string();
    let document_format = input["DocumentFormat"]
        .as_str()
        .unwrap_or("JSON")
        .to_string();

    let arn = build_document_arn(ctx, &name);
    let now = now_epoch_secs();

    let doc = SsmDocument {
        name: name.clone(),
        arn: arn.clone(),
        document_type: document_type.clone(),
        document_format: document_format.clone(),
        content: content.clone(),
        status: "Active".to_string(),
        document_version: "1".to_string(),
        created_date: now,
    };

    state.documents.insert(name.clone(), doc);

    Ok(json!({
        "DocumentDescription": {
            "Name": name,
            "DocumentVersion": "1",
            "Status": "Active",
            "DocumentType": document_type,
            "DocumentFormat": document_format,
            "CreatedDate": now,
        }
    }))
}

// ---------------------------------------------------------------------------
// GetDocument
// ---------------------------------------------------------------------------

pub fn get_document(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    let doc = state.documents.get(name).ok_or_else(|| {
        AwsError::bad_request(
            "InvalidDocument",
            format!("Document '{name}' does not exist"),
        )
    })?;

    Ok(json!({
        "Name": doc.name,
        "DocumentVersion": doc.document_version,
        "Content": doc.content,
        "DocumentType": doc.document_type,
        "DocumentFormat": doc.document_format,
        "Status": doc.status,
        "CreatedDate": doc.created_date,
    }))
}

// ---------------------------------------------------------------------------
// DescribeDocument
// ---------------------------------------------------------------------------

pub fn describe_document(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    let doc = state.documents.get(name).ok_or_else(|| {
        AwsError::bad_request(
            "InvalidDocument",
            format!("Document '{name}' does not exist"),
        )
    })?;

    Ok(json!({ "Document": document_description(&doc) }))
}

// ---------------------------------------------------------------------------
// UpdateDocument
// ---------------------------------------------------------------------------

pub fn update_document(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    let content = input["Content"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Content is required"))?;

    let mut doc = state.documents.get_mut(name).ok_or_else(|| {
        AwsError::bad_request(
            "InvalidDocument",
            format!("Document '{name}' does not exist"),
        )
    })?;

    doc.content = content.to_string();
    let new_version: u64 = doc.document_version.parse().unwrap_or(1) + 1;
    doc.document_version = new_version.to_string();

    let desc = document_description(&doc);
    drop(doc);

    Ok(json!({ "DocumentDescription": desc }))
}

// ---------------------------------------------------------------------------
// DeleteDocument
// ---------------------------------------------------------------------------

pub fn delete_document(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?;

    if state.documents.remove(name).is_none() {
        return Err(AwsError::bad_request(
            "InvalidDocument",
            format!("Document '{name}' does not exist"),
        ));
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListDocuments
// ---------------------------------------------------------------------------

pub fn list_documents(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let max_results = input["MaxResults"].as_u64().unwrap_or(50) as usize;

    let docs: Vec<Value> = state
        .documents
        .iter()
        .map(|e| document_description(e.value()))
        .take(max_results)
        .collect();

    Ok(json!({
        "DocumentIdentifiers": docs,
    }))
}

// ---------------------------------------------------------------------------
// CreateAssociation
// ---------------------------------------------------------------------------

pub fn create_association(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let document_name = input["Name"]
        .as_str()
        .ok_or_else(|| {
            AwsError::bad_request("InvalidParameter", "Name (document name) is required")
        })?
        .to_string();

    let targets = input["Targets"].as_array().cloned().unwrap_or_default();

    let association_id = Uuid::new_v4().to_string();
    let now = now_epoch_secs();

    let assoc = crate::state::SsmAssociation {
        association_id: association_id.clone(),
        name: association_id.clone(),
        document_name: document_name.clone(),
        targets: targets.clone(),
        status: "Pending".to_string(),
        created_date: now,
    };

    state.associations.insert(association_id.clone(), assoc);

    Ok(json!({
        "AssociationDescription": {
            "AssociationId": association_id,
            "Name": document_name,
            "Targets": targets,
            "Status": { "Name": "Pending" },
            "Date": now,
        }
    }))
}

// ---------------------------------------------------------------------------
// DescribeAssociation
// ---------------------------------------------------------------------------

pub fn describe_association(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let association_id = input["AssociationId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AssociationId is required"))?;

    let assoc = state.associations.get(association_id).ok_or_else(|| {
        AwsError::bad_request(
            "AssociationDoesNotExist",
            format!("Association '{association_id}' does not exist"),
        )
    })?;

    Ok(json!({
        "AssociationDescription": {
            "AssociationId": assoc.association_id,
            "Name": assoc.document_name,
            "Targets": assoc.targets,
            "Status": { "Name": assoc.status },
            "Date": assoc.created_date,
        }
    }))
}

// ---------------------------------------------------------------------------
// DeleteAssociation
// ---------------------------------------------------------------------------

pub fn delete_association(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let association_id = input["AssociationId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "AssociationId is required"))?;

    if state.associations.remove(association_id).is_none() {
        return Err(AwsError::bad_request(
            "AssociationDoesNotExist",
            format!("Association '{association_id}' does not exist"),
        ));
    }

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// ListAssociations
// ---------------------------------------------------------------------------

pub fn list_associations(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let max_results = input["MaxResults"].as_u64().unwrap_or(50) as usize;

    let assocs: Vec<Value> = state
        .associations
        .iter()
        .map(|e| {
            let a = e.value();
            json!({
                "AssociationId": a.association_id,
                "Name": a.document_name,
                "Targets": a.targets,
                "Status": { "Name": a.status },
                "Date": a.created_date,
            })
        })
        .take(max_results)
        .collect();

    Ok(json!({ "Associations": assocs }))
}

// ---------------------------------------------------------------------------
// CreateMaintenanceWindow
// ---------------------------------------------------------------------------

pub fn create_maintenance_window(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let name = input["Name"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Name is required"))?
        .to_string();

    let schedule = input["Schedule"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Schedule is required"))?
        .to_string();

    let duration = input["Duration"].as_u64().unwrap_or(1);
    let cutoff = input["Cutoff"].as_u64().unwrap_or(0);
    let allow_unassociated_targets = input["AllowUnassociatedTargets"].as_bool().unwrap_or(false);
    let _ = allow_unassociated_targets;

    let window_id = format!("mw-{}", &Uuid::new_v4().to_string().replace('-', "")[..16]);
    let now = now_epoch_secs();

    let window = crate::state::SsmMaintenanceWindow {
        window_id: window_id.clone(),
        name: name.clone(),
        schedule: schedule.clone(),
        duration,
        cutoff,
        enabled: true,
        created_date: now,
    };

    state.maintenance_windows.insert(window_id.clone(), window);

    Ok(json!({ "WindowId": window_id }))
}

// ---------------------------------------------------------------------------
// DescribeMaintenanceWindows
// ---------------------------------------------------------------------------

pub fn describe_maintenance_windows(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let max_results = input["MaxResults"].as_u64().unwrap_or(50) as usize;

    let windows: Vec<Value> = state
        .maintenance_windows
        .iter()
        .map(|e| {
            let w = e.value();
            json!({
                "WindowId": w.window_id,
                "Name": w.name,
                "Schedule": w.schedule,
                "Duration": w.duration,
                "Cutoff": w.cutoff,
                "Enabled": w.enabled,
            })
        })
        .take(max_results)
        .collect();

    Ok(json!({ "WindowIdentities": windows }))
}

// ---------------------------------------------------------------------------
// DeleteMaintenanceWindow
// ---------------------------------------------------------------------------

pub fn delete_maintenance_window(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let window_id = input["WindowId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "WindowId is required"))?;

    state.maintenance_windows.remove(window_id);

    Ok(json!({ "WindowId": window_id }))
}

// ---------------------------------------------------------------------------
// CreateOpsItem
// ---------------------------------------------------------------------------

pub fn create_ops_item(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let title = input["Title"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "Title is required"))?
        .to_string();

    let description = input["Description"].as_str().unwrap_or("").to_string();
    let severity = input["Severity"].as_str().unwrap_or("3").to_string();

    let ops_item_id = format!("oi-{}", &Uuid::new_v4().to_string().replace('-', "")[..16]);
    let now = now_epoch_secs();

    let item = crate::state::SsmOpsItem {
        ops_item_id: ops_item_id.clone(),
        title,
        description,
        status: "Open".to_string(),
        severity,
        created_time: now,
        last_modified_time: now,
    };

    state.ops_items.insert(ops_item_id.clone(), item);

    Ok(json!({ "OpsItemId": ops_item_id }))
}

// ---------------------------------------------------------------------------
// GetOpsItem
// ---------------------------------------------------------------------------

pub fn get_ops_item(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ops_item_id = input["OpsItemId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "OpsItemId is required"))?;

    let item = state.ops_items.get(ops_item_id).ok_or_else(|| {
        AwsError::bad_request(
            "OpsItemNotFoundException",
            format!("OpsItem '{ops_item_id}' does not exist"),
        )
    })?;

    Ok(json!({
        "OpsItem": {
            "OpsItemId": item.ops_item_id,
            "Title": item.title,
            "Description": item.description,
            "Status": item.status,
            "Severity": item.severity,
            "CreatedTime": item.created_time,
            "LastModifiedTime": item.last_modified_time,
        }
    }))
}

// ---------------------------------------------------------------------------
// UpdateOpsItem
// ---------------------------------------------------------------------------

pub fn update_ops_item(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let ops_item_id = input["OpsItemId"]
        .as_str()
        .ok_or_else(|| AwsError::bad_request("InvalidParameter", "OpsItemId is required"))?;

    let mut item = state.ops_items.get_mut(ops_item_id).ok_or_else(|| {
        AwsError::bad_request(
            "OpsItemNotFoundException",
            format!("OpsItem '{ops_item_id}' does not exist"),
        )
    })?;

    if let Some(title) = input["Title"].as_str() {
        item.title = title.to_string();
    }
    if let Some(description) = input["Description"].as_str() {
        item.description = description.to_string();
    }
    if let Some(status) = input["Status"].as_str() {
        item.status = status.to_string();
    }
    if let Some(severity) = input["Severity"].as_str() {
        item.severity = severity.to_string();
    }

    item.last_modified_time = now_epoch_secs();

    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// DescribeOpsItems
// ---------------------------------------------------------------------------

pub fn describe_ops_items(
    state: &SsmState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let max_results = input["MaxResults"].as_u64().unwrap_or(50) as usize;

    let items: Vec<Value> = state
        .ops_items
        .iter()
        .map(|e| {
            let i = e.value();
            json!({
                "OpsItemId": i.ops_item_id,
                "Title": i.title,
                "Status": i.status,
                "Severity": i.severity,
                "CreatedTime": i.created_time,
                "LastModifiedTime": i.last_modified_time,
            })
        })
        .take(max_results)
        .collect();

    Ok(json!({ "OpsItemSummaries": items }))
}
