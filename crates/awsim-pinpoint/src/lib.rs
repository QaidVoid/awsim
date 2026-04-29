//! Amazon Pinpoint emulator. Apps (projects), endpoints, segments, campaigns.
//! No actual messaging is sent — campaigns "complete" immediately.

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
pub struct PinpointState {
    pub apps: DashMap<String, App>,
    /// (app_id, endpoint_id) keyed.
    pub endpoints: DashMap<String, Endpoint>,
    /// (app_id, segment_id) keyed.
    pub segments: DashMap<String, Segment>,
    /// (app_id, campaign_id) keyed.
    pub campaigns: DashMap<String, Campaign>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub id: String,
    pub name: String,
    pub arn: String,
    pub creation_date: String,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: String,
    pub application_id: String,
    pub address: Option<String>,
    pub channel_type: Option<String>,
    pub effective_date: String,
    pub endpoint_status: String,
    pub user: Option<Value>,
    pub attributes: Option<Value>,
    pub demographic: Option<Value>,
    pub location: Option<Value>,
    pub metrics: Option<Value>,
    pub opt_out: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub id: String,
    pub application_id: String,
    pub arn: String,
    pub name: String,
    pub creation_date: String,
    pub last_modified_date: String,
    pub segment_type: String,
    pub version: u32,
    pub dimensions: Option<Value>,
    pub segment_groups: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Campaign {
    pub id: String,
    pub application_id: String,
    pub arn: String,
    pub name: String,
    pub state: String,
    pub creation_date: String,
    pub last_modified_date: String,
    pub segment_id: String,
    pub segment_version: u32,
    pub message_configuration: Option<Value>,
    pub schedule: Option<Value>,
    pub version: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PinpointSnapshot {
    pub apps: Vec<App>,
    pub endpoints: Vec<Endpoint>,
    pub segments: Vec<Segment>,
    pub campaigns: Vec<Campaign>,
}

fn endpoint_key(app_id: &str, endpoint_id: &str) -> String {
    format!("{app_id}|{endpoint_id}")
}
fn segment_key(app_id: &str, segment_id: &str) -> String {
    format!("{app_id}|{segment_id}")
}
fn campaign_key(app_id: &str, campaign_id: &str) -> String {
    format!("{app_id}|{campaign_id}")
}

impl PinpointState {
    pub fn to_snapshot(&self) -> PinpointSnapshot {
        PinpointSnapshot {
            apps: self.apps.iter().map(|e| e.value().clone()).collect(),
            endpoints: self.endpoints.iter().map(|e| e.value().clone()).collect(),
            segments: self.segments.iter().map(|e| e.value().clone()).collect(),
            campaigns: self.campaigns.iter().map(|e| e.value().clone()).collect(),
        }
    }
    pub fn restore_from_snapshot(&self, snap: PinpointSnapshot) {
        self.apps.clear();
        self.endpoints.clear();
        self.segments.clear();
        self.campaigns.clear();
        for a in snap.apps {
            self.apps.insert(a.id.clone(), a);
        }
        for e in snap.endpoints {
            self.endpoints
                .insert(endpoint_key(&e.application_id, &e.id), e);
        }
        for s in snap.segments {
            self.segments
                .insert(segment_key(&s.application_id, &s.id), s);
        }
        for c in snap.campaigns {
            self.campaigns
                .insert(campaign_key(&c.application_id, &c.id), c);
        }
    }
}

fn now_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{secs}")
}

fn require_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AwsError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| AwsError::bad_request("BadRequestException", format!("{key} is required")))
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn app_arn(ctx: &RequestContext, id: &str) -> String {
    format!(
        "arn:aws:mobiletargeting:{}:{}:apps/{}",
        ctx.region, ctx.account_id, id
    )
}

fn segment_arn(ctx: &RequestContext, app: &str, id: &str) -> String {
    format!(
        "arn:aws:mobiletargeting:{}:{}:apps/{}/segments/{}",
        ctx.region, ctx.account_id, app, id
    )
}

fn campaign_arn(ctx: &RequestContext, app: &str, id: &str) -> String {
    format!(
        "arn:aws:mobiletargeting:{}:{}:apps/{}/campaigns/{}",
        ctx.region, ctx.account_id, app, id
    )
}

fn app_to_value(a: &App) -> Value {
    json!({
        "Id": a.id,
        "Name": a.name,
        "Arn": a.arn,
        "CreationDate": a.creation_date,
        "tags": a.tags,
    })
}

fn endpoint_to_value(e: &Endpoint) -> Value {
    json!({
        "Id": e.id,
        "ApplicationId": e.application_id,
        "Address": e.address,
        "ChannelType": e.channel_type,
        "EffectiveDate": e.effective_date,
        "EndpointStatus": e.endpoint_status,
        "User": e.user,
        "Attributes": e.attributes,
        "Demographic": e.demographic,
        "Location": e.location,
        "Metrics": e.metrics,
        "OptOut": e.opt_out,
    })
}

fn segment_to_value(s: &Segment) -> Value {
    json!({
        "Id": s.id,
        "ApplicationId": s.application_id,
        "Arn": s.arn,
        "Name": s.name,
        "CreationDate": s.creation_date,
        "LastModifiedDate": s.last_modified_date,
        "SegmentType": s.segment_type,
        "Version": s.version,
        "Dimensions": s.dimensions,
        "SegmentGroups": s.segment_groups,
    })
}

fn campaign_to_value(c: &Campaign) -> Value {
    json!({
        "Id": c.id,
        "ApplicationId": c.application_id,
        "Arn": c.arn,
        "Name": c.name,
        "State": { "CampaignStatus": c.state },
        "CreationDate": c.creation_date,
        "LastModifiedDate": c.last_modified_date,
        "SegmentId": c.segment_id,
        "SegmentVersion": c.segment_version,
        "MessageConfiguration": c.message_configuration,
        "Schedule": c.schedule,
        "Version": c.version,
    })
}

pub struct PinpointService {
    store: AccountRegionStore<PinpointState>,
}

impl PinpointService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    pub fn store(&self) -> AccountRegionStore<PinpointState> {
        self.store.clone()
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<PinpointState> {
        self.store.get(&ctx.account_id, &ctx.region)
    }
}

impl Default for PinpointService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for PinpointService {
    fn service_name(&self) -> &str {
        "mobiletargeting"
    }

    fn signing_name(&self) -> &str {
        "mobiletargeting"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            // Apps
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/apps",
                operation: "CreateApp",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apps",
                operation: "GetApps",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apps/{ApplicationId}",
                operation: "GetApp",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/apps/{ApplicationId}",
                operation: "DeleteApp",
                required_query_param: None,
            },
            // Endpoints
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v1/apps/{ApplicationId}/endpoints/{EndpointId}",
                operation: "UpdateEndpoint",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apps/{ApplicationId}/endpoints/{EndpointId}",
                operation: "GetEndpoint",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/apps/{ApplicationId}/endpoints/{EndpointId}",
                operation: "DeleteEndpoint",
                required_query_param: None,
            },
            // Segments
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/apps/{ApplicationId}/segments",
                operation: "CreateSegment",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apps/{ApplicationId}/segments",
                operation: "GetSegments",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apps/{ApplicationId}/segments/{SegmentId}",
                operation: "GetSegment",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/apps/{ApplicationId}/segments/{SegmentId}",
                operation: "DeleteSegment",
                required_query_param: None,
            },
            // Campaigns
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/apps/{ApplicationId}/campaigns",
                operation: "CreateCampaign",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apps/{ApplicationId}/campaigns",
                operation: "GetCampaigns",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/apps/{ApplicationId}/campaigns/{CampaignId}",
                operation: "GetCampaign",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/apps/{ApplicationId}/campaigns/{CampaignId}",
                operation: "DeleteCampaign",
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
        debug!(operation, "Pinpoint request");
        let state = self.get_state(ctx);
        match operation {
            "CreateApp" => {
                let req = input.get("CreateApplicationRequest").unwrap_or(&input);
                let name = req
                    .get("Name")
                    .or_else(|| input.get("Name"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AwsError::bad_request("BadRequestException", "Name is required")
                    })?
                    .to_string();
                let id = new_id();
                let tags = req
                    .get("tags")
                    .or_else(|| input.get("tags"))
                    .and_then(|v| v.as_object())
                    .map(|o| {
                        o.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect()
                    })
                    .unwrap_or_default();
                let a = App {
                    id: id.clone(),
                    name,
                    arn: app_arn(ctx, &id),
                    creation_date: now_iso(),
                    tags,
                };
                let result = json!({ "ApplicationResponse": app_to_value(&a) });
                state.apps.insert(id, a);
                Ok(result)
            }
            "GetApp" => {
                let id = require_str(&input, "ApplicationId")?;
                let a = state.apps.get(id).ok_or_else(|| {
                    AwsError::not_found("NotFoundException", format!("Application {id} not found"))
                })?;
                Ok(json!({ "ApplicationResponse": app_to_value(&a) }))
            }
            "GetApps" => {
                let items: Vec<Value> =
                    state.apps.iter().map(|e| app_to_value(e.value())).collect();
                Ok(json!({ "ApplicationsResponse": { "Item": items } }))
            }
            "DeleteApp" => {
                let id = require_str(&input, "ApplicationId")?.to_string();
                let (_, a) = state.apps.remove(&id).ok_or_else(|| {
                    AwsError::not_found("NotFoundException", format!("Application {id} not found"))
                })?;
                let prefix = format!("{id}|");
                state.endpoints.retain(|k, _| !k.starts_with(&prefix));
                state.segments.retain(|k, _| !k.starts_with(&prefix));
                state.campaigns.retain(|k, _| !k.starts_with(&prefix));
                Ok(json!({ "ApplicationResponse": app_to_value(&a) }))
            }
            "UpdateEndpoint" => {
                let app_id = require_str(&input, "ApplicationId")?.to_string();
                let endpoint_id = require_str(&input, "EndpointId")?.to_string();
                if !state.apps.contains_key(&app_id) {
                    return Err(AwsError::not_found(
                        "NotFoundException",
                        format!("Application {app_id} not found"),
                    ));
                }
                let req = input
                    .get("EndpointRequest")
                    .cloned()
                    .unwrap_or(Value::Object(Default::default()));
                let e = Endpoint {
                    id: endpoint_id.clone(),
                    application_id: app_id.clone(),
                    address: req
                        .get("Address")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    channel_type: req
                        .get("ChannelType")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    effective_date: now_iso(),
                    endpoint_status: req
                        .get("EndpointStatus")
                        .and_then(|v| v.as_str())
                        .unwrap_or("ACTIVE")
                        .to_string(),
                    user: req.get("User").cloned(),
                    attributes: req.get("Attributes").cloned(),
                    demographic: req.get("Demographic").cloned(),
                    location: req.get("Location").cloned(),
                    metrics: req.get("Metrics").cloned(),
                    opt_out: req
                        .get("OptOut")
                        .and_then(|v| v.as_str())
                        .unwrap_or("NONE")
                        .to_string(),
                };
                state
                    .endpoints
                    .insert(endpoint_key(&app_id, &endpoint_id), e);
                Ok(
                    json!({ "MessageBody": { "Message": "Accepted", "RequestID": uuid::Uuid::new_v4().to_string() } }),
                )
            }
            "GetEndpoint" => {
                let app_id = require_str(&input, "ApplicationId")?;
                let endpoint_id = require_str(&input, "EndpointId")?;
                let e = state
                    .endpoints
                    .get(&endpoint_key(app_id, endpoint_id))
                    .ok_or_else(|| {
                        AwsError::not_found(
                            "NotFoundException",
                            format!("Endpoint {endpoint_id} not found"),
                        )
                    })?;
                Ok(json!({ "EndpointResponse": endpoint_to_value(&e) }))
            }
            "DeleteEndpoint" => {
                let app_id = require_str(&input, "ApplicationId")?.to_string();
                let endpoint_id = require_str(&input, "EndpointId")?.to_string();
                let (_, e) = state
                    .endpoints
                    .remove(&endpoint_key(&app_id, &endpoint_id))
                    .ok_or_else(|| {
                        AwsError::not_found(
                            "NotFoundException",
                            format!("Endpoint {endpoint_id} not found"),
                        )
                    })?;
                Ok(json!({ "EndpointResponse": endpoint_to_value(&e) }))
            }
            "CreateSegment" => {
                let app_id = require_str(&input, "ApplicationId")?.to_string();
                if !state.apps.contains_key(&app_id) {
                    return Err(AwsError::not_found(
                        "NotFoundException",
                        format!("Application {app_id} not found"),
                    ));
                }
                let req = input
                    .get("WriteSegmentRequest")
                    .cloned()
                    .unwrap_or(Value::Object(Default::default()));
                let id = new_id();
                let s = Segment {
                    id: id.clone(),
                    application_id: app_id.clone(),
                    arn: segment_arn(ctx, &app_id, &id),
                    name: req
                        .get("Name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("default-segment")
                        .to_string(),
                    creation_date: now_iso(),
                    last_modified_date: now_iso(),
                    segment_type: "DIMENSIONAL".to_string(),
                    version: 1,
                    dimensions: req.get("Dimensions").cloned(),
                    segment_groups: req.get("SegmentGroups").cloned(),
                };
                let result = json!({ "SegmentResponse": segment_to_value(&s) });
                state.segments.insert(segment_key(&app_id, &id), s);
                Ok(result)
            }
            "GetSegment" => {
                let app_id = require_str(&input, "ApplicationId")?;
                let segment_id = require_str(&input, "SegmentId")?;
                let s = state
                    .segments
                    .get(&segment_key(app_id, segment_id))
                    .ok_or_else(|| {
                        AwsError::not_found(
                            "NotFoundException",
                            format!("Segment {segment_id} not found"),
                        )
                    })?;
                Ok(json!({ "SegmentResponse": segment_to_value(&s) }))
            }
            "GetSegments" => {
                let app_id = require_str(&input, "ApplicationId")?;
                let items: Vec<Value> = state
                    .segments
                    .iter()
                    .filter(|e| e.value().application_id == app_id)
                    .map(|e| segment_to_value(e.value()))
                    .collect();
                Ok(json!({ "SegmentsResponse": { "Item": items } }))
            }
            "DeleteSegment" => {
                let app_id = require_str(&input, "ApplicationId")?;
                let segment_id = require_str(&input, "SegmentId")?;
                let (_, s) = state
                    .segments
                    .remove(&segment_key(app_id, segment_id))
                    .ok_or_else(|| {
                        AwsError::not_found(
                            "NotFoundException",
                            format!("Segment {segment_id} not found"),
                        )
                    })?;
                Ok(json!({ "SegmentResponse": segment_to_value(&s) }))
            }
            "CreateCampaign" => {
                let app_id = require_str(&input, "ApplicationId")?.to_string();
                if !state.apps.contains_key(&app_id) {
                    return Err(AwsError::not_found(
                        "NotFoundException",
                        format!("Application {app_id} not found"),
                    ));
                }
                let req = input
                    .get("WriteCampaignRequest")
                    .cloned()
                    .unwrap_or(Value::Object(Default::default()));
                let id = new_id();
                let c = Campaign {
                    id: id.clone(),
                    application_id: app_id.clone(),
                    arn: campaign_arn(ctx, &app_id, &id),
                    name: req
                        .get("Name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("default-campaign")
                        .to_string(),
                    state: "COMPLETED".to_string(),
                    creation_date: now_iso(),
                    last_modified_date: now_iso(),
                    segment_id: req
                        .get("SegmentId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    segment_version: req
                        .get("SegmentVersion")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1) as u32,
                    message_configuration: req.get("MessageConfiguration").cloned(),
                    schedule: req.get("Schedule").cloned(),
                    version: 1,
                };
                let result = json!({ "CampaignResponse": campaign_to_value(&c) });
                state.campaigns.insert(campaign_key(&app_id, &id), c);
                Ok(result)
            }
            "GetCampaign" => {
                let app_id = require_str(&input, "ApplicationId")?;
                let campaign_id = require_str(&input, "CampaignId")?;
                let c = state
                    .campaigns
                    .get(&campaign_key(app_id, campaign_id))
                    .ok_or_else(|| {
                        AwsError::not_found(
                            "NotFoundException",
                            format!("Campaign {campaign_id} not found"),
                        )
                    })?;
                Ok(json!({ "CampaignResponse": campaign_to_value(&c) }))
            }
            "GetCampaigns" => {
                let app_id = require_str(&input, "ApplicationId")?;
                let items: Vec<Value> = state
                    .campaigns
                    .iter()
                    .filter(|e| e.value().application_id == app_id)
                    .map(|e| campaign_to_value(e.value()))
                    .collect();
                Ok(json!({ "CampaignsResponse": { "Item": items } }))
            }
            "DeleteCampaign" => {
                let app_id = require_str(&input, "ApplicationId")?;
                let campaign_id = require_str(&input, "CampaignId")?;
                let (_, c) = state
                    .campaigns
                    .remove(&campaign_key(app_id, campaign_id))
                    .ok_or_else(|| {
                        AwsError::not_found(
                            "NotFoundException",
                            format!("Campaign {campaign_id} not found"),
                        )
                    })?;
                Ok(json!({ "CampaignResponse": campaign_to_value(&c) }))
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }

    fn snapshot(&self) -> Option<Vec<u8>> {
        let mut all = PinpointSnapshot {
            apps: vec![],
            endpoints: vec![],
            segments: vec![],
            campaigns: vec![],
        };
        for (_, st) in self.store.iter_all() {
            let s = st.to_snapshot();
            all.apps.extend(s.apps);
            all.endpoints.extend(s.endpoints);
            all.segments.extend(s.segments);
            all.campaigns.extend(s.campaigns);
        }
        serde_json::to_vec(&all).ok()
    }

    fn restore(&self, data: &[u8]) -> Result<(), String> {
        let snap: PinpointSnapshot = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let st = self.store.get("000000000000", "us-east-1");
        st.restore_from_snapshot(snap);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("mobiletargeting", "us-east-1")
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
    fn app_endpoint_segment_campaign_lifecycle() {
        let svc = PinpointService::new();
        let ctx = ctx();
        let a = block_on(svc.handle(
            "CreateApp",
            json!({ "CreateApplicationRequest": { "Name": "marketing" } }),
            &ctx,
        ))
        .unwrap();
        let app_id = a["ApplicationResponse"]["Id"].as_str().unwrap().to_string();

        block_on(svc.handle(
            "UpdateEndpoint",
            json!({
                "ApplicationId": app_id,
                "EndpointId": "device-1",
                "EndpointRequest": { "ChannelType": "EMAIL", "Address": "alice@example.com" }
            }),
            &ctx,
        ))
        .unwrap();
        let endpoint = block_on(svc.handle(
            "GetEndpoint",
            json!({ "ApplicationId": app_id, "EndpointId": "device-1" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(endpoint["EndpointResponse"]["Address"], "alice@example.com");

        let s = block_on(svc.handle(
            "CreateSegment",
            json!({ "ApplicationId": app_id, "WriteSegmentRequest": { "Name": "all" } }),
            &ctx,
        ))
        .unwrap();
        let seg_id = s["SegmentResponse"]["Id"].as_str().unwrap().to_string();

        let c = block_on(svc.handle(
            "CreateCampaign",
            json!({
                "ApplicationId": app_id,
                "WriteCampaignRequest": {
                    "Name": "welcome",
                    "SegmentId": seg_id,
                    "MessageConfiguration": { "EmailMessage": { "Body": "Hi" } }
                }
            }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(
            c["CampaignResponse"]["State"]["CampaignStatus"],
            "COMPLETED"
        );

        block_on(svc.handle("DeleteApp", json!({ "ApplicationId": app_id }), &ctx)).unwrap();
    }
}
