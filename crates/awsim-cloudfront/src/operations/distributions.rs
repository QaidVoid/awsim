use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{
    ids::{distribution_arn, distribution_domain, new_distribution_id, new_etag, now_iso8601},
    state::{CloudFrontState, Distribution, DistributionConfig, Origin},
};

fn not_found(id: &str) -> AwsError {
    AwsError::not_found(
        "NoSuchDistribution",
        format!("The specified distribution does not exist: {id}"),
    )
}

fn parse_config(input: &Value) -> DistributionConfig {
    let config = input
        .get("DistributionConfig")
        .unwrap_or(input);

    let comment = config
        .get("Comment")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let enabled = config
        .get("Enabled")
        .and_then(|v| match v {
            Value::Bool(b) => Some(*b),
            Value::String(s) => Some(s == "true" || s == "True"),
            _ => None,
        })
        .unwrap_or(true);

    let price_class = config
        .get("PriceClass")
        .and_then(|v| v.as_str())
        .unwrap_or("PriceClass_All")
        .to_string();

    let http_version = config
        .get("HttpVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("http2")
        .to_string();

    let is_ipv6_enabled = config
        .get("IsIPV6Enabled")
        .and_then(|v| match v {
            Value::Bool(b) => Some(*b),
            Value::String(s) => Some(s == "true"),
            _ => None,
        })
        .unwrap_or(false);

    let default_cache_behavior = config
        .get("DefaultCacheBehavior")
        .cloned()
        .unwrap_or(json!({
            "ViewerProtocolPolicy": "redirect-to-https",
            "AllowedMethods": { "Quantity": 2, "Items": { "Method": ["GET", "HEAD"] } },
            "CachePolicyId": "658327ea-f89d-4fab-a63d-7e88639e58f6",
            "Compress": true,
        }));

    let origins = parse_origins(config);

    DistributionConfig {
        origins,
        default_cache_behavior,
        comment,
        enabled,
        price_class,
        http_version,
        is_ipv6_enabled,
    }
}

fn parse_origins(config: &Value) -> Vec<Origin> {
    let mut origins = Vec::new();

    if let Some(origins_val) = config.get("Origins") {
        // Could be wrapped in Items.Origin or Items or directly an array
        let items = origins_val
            .get("Items")
            .and_then(|v| v.get("Origin"))
            .or_else(|| origins_val.get("Items"))
            .unwrap_or(origins_val);

        let origin_list: Vec<&Value> = match items {
            Value::Array(arr) => arr.iter().collect(),
            Value::Object(_) => vec![items],
            _ => vec![],
        };

        for o in origin_list {
            let id = o
                .get("Id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let domain = o
                .get("DomainName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let s3_origin_config = o.get("S3OriginConfig").cloned();
            let custom_origin_config = o.get("CustomOriginConfig").cloned();

            origins.push(Origin {
                id,
                domain_name: domain,
                s3_origin_config,
                custom_origin_config,
            });
        }
    }

    origins
}

fn distribution_to_value(d: &Distribution) -> Value {
    let origins_list: Vec<Value> = d
        .config
        .origins
        .iter()
        .map(|o| {
            let mut v = json!({
                "Id": o.id,
                "DomainName": o.domain_name,
            });
            if let Some(ref s3) = o.s3_origin_config {
                v["S3OriginConfig"] = s3.clone();
            }
            if let Some(ref custom) = o.custom_origin_config {
                v["CustomOriginConfig"] = custom.clone();
            }
            v
        })
        .collect();

    let qty = origins_list.len();

    json!({
        "Id": d.id,
        "ARN": d.arn,
        "Status": d.status,
        "DomainName": d.domain_name,
        "LastModifiedTime": d.created_at,
        "DistributionConfig": {
            "CallerReference": d.id,
            "Origins": {
                "Quantity": qty,
                "Items": { "Origin": origins_list }
            },
            "DefaultCacheBehavior": d.config.default_cache_behavior,
            "Comment": d.config.comment,
            "Enabled": d.config.enabled,
            "PriceClass": d.config.price_class,
            "HttpVersion": d.config.http_version,
            "IsIPV6Enabled": d.config.is_ipv6_enabled,
        },
        "ActiveTrustedSigners": { "Enabled": false, "Quantity": 0 },
        "ActiveTrustedKeyGroups": { "Enabled": false, "Quantity": 0 },
    })
}

pub fn create_distribution(
    state: &CloudFrontState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let id = new_distribution_id();
    let arn = distribution_arn(&ctx.account_id, &id);
    let domain_name = distribution_domain(&id);
    let config = parse_config(input);
    let etag = new_etag();

    let dist = Distribution {
        id: id.clone(),
        arn,
        domain_name,
        status: "Deployed".to_string(),
        config,
        created_at: now_iso8601(),
        tags: HashMap::new(),
        etag: etag.clone(),
    };

    let result = distribution_to_value(&dist);
    state.distributions.insert(id, dist);

    Ok(json!({
        "Distribution": result,
        "Location": format!("https://cloudfront.amazonaws.com/2020-05-31/distribution/{}", result["Id"].as_str().unwrap_or("")),
        "ETag": etag,
    }))
}

pub fn get_distribution(state: &CloudFrontState, id: &str) -> Result<Value, AwsError> {
    let dist = state
        .distributions
        .get(id)
        .ok_or_else(|| not_found(id))?;

    let etag = dist.etag.clone();
    let result = distribution_to_value(&dist);

    Ok(json!({
        "Distribution": result,
        "ETag": etag,
    }))
}

pub fn list_distributions(state: &CloudFrontState) -> Result<Value, AwsError> {
    let items: Vec<Value> = state
        .distributions
        .iter()
        .map(|e| distribution_to_value(e.value()))
        .collect();

    let qty = items.len();

    Ok(json!({
        "DistributionList": {
            "Marker": "",
            "MaxItems": 100,
            "IsTruncated": false,
            "Quantity": qty,
            "Items": { "DistributionSummary": items }
        }
    }))
}

pub fn delete_distribution(state: &CloudFrontState, id: &str) -> Result<Value, AwsError> {
    if state.distributions.remove(id).is_none() {
        return Err(not_found(id));
    }

    Ok(json!({}))
}

pub fn update_distribution(
    state: &CloudFrontState,
    id: &str,
    input: &Value,
) -> Result<Value, AwsError> {
    let mut dist = state
        .distributions
        .get_mut(id)
        .ok_or_else(|| not_found(id))?;

    dist.config = parse_config(input);
    let etag = new_etag();
    dist.etag = etag.clone();

    let result = distribution_to_value(&dist);

    Ok(json!({
        "Distribution": result,
        "ETag": etag,
    }))
}
