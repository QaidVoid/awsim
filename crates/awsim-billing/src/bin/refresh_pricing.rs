//! Pull current AWS pricing for the services AWSim meters and emit slim
//! JSON files into `crates/awsim-billing/pricing/`. Run after AWS
//! publishes a price change (rare) or to bring vendored data forward:
//!
//!   cargo run -p awsim-billing --bin refresh-pricing --features refresh
//!
//! AWS publishes per-service pricing as huge JSON files indexed by an
//! opaque SKU. For each (productFamily, usagetype) we model, we look up
//! the matching SKU, pull its OnDemand $/USD rate and AWS-supplied
//! description, and emit a slim file pairing those with our operation
//! → dimension map (the one piece AWS doesn't publish: AWS describes
//! "Tier1" as English text "PUT/COPY/POST/LIST requests", not as a
//! machine-readable list of operation names).
//!
//! Outbound transfer rate comes from the AWSDataTransfer offer because
//! per-service files don't include internet egress pricing.

use awsim_billing::{RequestDimension, ServicePricing};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

const REGION: &str = "us-east-1";
const REGION_DISPLAY: &str = "US East (N. Virginia)";
const BASE: &str = "https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws";

/// One service's refresh recipe.
struct ServiceConfig {
    /// Signing name we emit (e.g. "s3"); used as the JSON `service`
    /// field and the output filename stem.
    service: &'static str,
    /// AWS offer code (e.g. "AmazonS3").
    aws_code: &'static str,
    /// Fallback per-request rate for ops not matched by any dimension.
    default_request_rate: f64,
    dimensions: &'static [DimensionConfig],
}

struct DimensionConfig {
    /// Operations that fall under this dimension. Project knowledge —
    /// AWS doesn't publish this map.
    operations: &'static [&'static str],
    /// How to find the AWS SKU for this dimension's rate. `None` means
    /// "AWS doesn't bill this; emit the row at fixed_rate".
    matcher: Option<DimensionMatcher>,
    /// Used when `matcher` is `None`. Description shown in the bill.
    fixed_description: &'static str,
    /// Used when `matcher` is `None`.
    fixed_rate: f64,
}

struct DimensionMatcher {
    product_family: &'static str,
    /// Predicate over the AWS product's `attributes` map. The first
    /// product whose attributes satisfy *all* (key, value) pairs wins.
    attributes: &'static [(&'static str, &'static str)],
}

const SERVICES: &[ServiceConfig] = &[
    ServiceConfig {
        service: "s3",
        aws_code: "AmazonS3",
        default_request_rate: 4.0e-7,
        dimensions: &[
            DimensionConfig {
                operations: &[
                    "PutObject",
                    "CopyObject",
                    "PostObject",
                    "ListObjects",
                    "ListObjectsV2",
                    "ListObjectVersions",
                    "ListBuckets",
                    "ListMultipartUploads",
                    "ListParts",
                    "CreateBucket",
                    "CreateMultipartUpload",
                    "UploadPart",
                    "UploadPartCopy",
                    "CompleteMultipartUpload",
                    "AbortMultipartUpload",
                    "PutBucketAcl",
                    "PutBucketPolicy",
                    "PutBucketTagging",
                    "PutBucketVersioning",
                    "PutBucketLifecycleConfiguration",
                    "PutBucketCors",
                    "PutBucketEncryption",
                    "PutBucketNotificationConfiguration",
                    "PutBucketWebsite",
                    "PutBucketLogging",
                    "PutBucketReplication",
                    "PutBucketOwnershipControls",
                    "PutBucketRequestPayment",
                    "PutBucketAccelerateConfiguration",
                    "PutBucketIntelligentTieringConfiguration",
                    "PutBucketAnalyticsConfiguration",
                    "PutBucketInventoryConfiguration",
                    "PutBucketMetricsConfiguration",
                    "PutObjectAcl",
                    "PutObjectTagging",
                    "PutObjectLegalHold",
                    "PutObjectRetention",
                    "PutObjectLockConfiguration",
                    "RestoreObject",
                    "WriteGetObjectResponse",
                ],
                matcher: Some(DimensionMatcher {
                    product_family: "API Request",
                    attributes: &[("usagetype", "Requests-Tier1")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "GetObject",
                    "HeadObject",
                    "HeadBucket",
                    "GetObjectAcl",
                    "GetObjectTagging",
                    "GetObjectAttributes",
                    "GetBucketAcl",
                    "GetBucketPolicy",
                    "GetBucketLocation",
                    "GetBucketTagging",
                    "GetBucketVersioning",
                    "GetBucketLifecycleConfiguration",
                    "GetBucketCors",
                    "GetBucketEncryption",
                    "GetBucketNotificationConfiguration",
                    "SelectObjectContent",
                ],
                matcher: Some(DimensionMatcher {
                    product_family: "API Request",
                    attributes: &[("usagetype", "Requests-Tier2")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "DeleteObject",
                    "DeleteObjects",
                    "DeleteBucket",
                    "DeleteBucketPolicy",
                    "DeleteBucketTagging",
                    "DeleteBucketLifecycle",
                    "DeleteBucketCors",
                    "DeleteBucketEncryption",
                ],
                matcher: None,
                fixed_description: "Delete and Cancel requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "lambda",
        aws_code: "AWSLambda",
        default_request_rate: 0.0,
        dimensions: &[
            DimensionConfig {
                operations: &["Invoke", "InvokeAsync", "InvokeWithResponseStream"],
                matcher: Some(DimensionMatcher {
                    product_family: "Serverless",
                    attributes: &[("usagetype", "Request"), ("group", "AWS-Lambda-Requests")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "CreateFunction",
                    "UpdateFunctionCode",
                    "UpdateFunctionConfiguration",
                    "DeleteFunction",
                    "GetFunction",
                    "GetFunctionConfiguration",
                    "ListFunctions",
                    "PublishVersion",
                    "CreateAlias",
                    "UpdateAlias",
                    "DeleteAlias",
                    "GetAlias",
                    "ListAliases",
                    "AddPermission",
                    "RemovePermission",
                    "GetPolicy",
                    "PutFunctionConcurrency",
                    "DeleteFunctionConcurrency",
                    "GetFunctionConcurrency",
                    "PutProvisionedConcurrencyConfig",
                    "DeleteProvisionedConcurrencyConfig",
                    "GetProvisionedConcurrencyConfig",
                    "ListProvisionedConcurrencyConfigs",
                    "TagResource",
                    "UntagResource",
                    "ListTags",
                    "CreateEventSourceMapping",
                    "UpdateEventSourceMapping",
                    "DeleteEventSourceMapping",
                    "ListEventSourceMappings",
                    "GetEventSourceMapping",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "dynamodb",
        aws_code: "AmazonDynamoDB",
        default_request_rate: 0.0,
        dimensions: &[
            DimensionConfig {
                operations: &[
                    "PutItem",
                    "UpdateItem",
                    "DeleteItem",
                    "BatchWriteItem",
                    "TransactWriteItems",
                ],
                matcher: Some(DimensionMatcher {
                    product_family: "Amazon DynamoDB PayPerRequest Throughput",
                    attributes: &[("group", "DDB-WriteUnits")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "GetItem",
                    "BatchGetItem",
                    "Query",
                    "Scan",
                    "TransactGetItems",
                ],
                matcher: Some(DimensionMatcher {
                    product_family: "Amazon DynamoDB PayPerRequest Throughput",
                    attributes: &[("group", "DDB-ReadUnits")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "CreateTable",
                    "DeleteTable",
                    "DescribeTable",
                    "ListTables",
                    "UpdateTable",
                    "TagResource",
                    "UntagResource",
                    "ListTagsOfResource",
                    "DescribeLimits",
                    "DescribeContinuousBackups",
                    "UpdateContinuousBackups",
                    "CreateBackup",
                    "DeleteBackup",
                    "ListBackups",
                    "DescribeBackup",
                    "RestoreTableFromBackup",
                    "RestoreTableToPointInTime",
                    "CreateGlobalTable",
                    "UpdateGlobalTable",
                    "DescribeGlobalTable",
                    "ListGlobalTables",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
];

/// Decoded AWS bulk pricing JSON. We only deserialize the fields we
/// touch; everything else stays as `Value`.
#[derive(Debug, Deserialize)]
struct PricingDoc {
    #[serde(rename = "publicationDate")]
    publication_date: Option<String>,
    products: HashMap<String, Product>,
    terms: Terms,
}

#[derive(Debug, Deserialize)]
struct Product {
    #[allow(dead_code)]
    sku: String,
    #[serde(rename = "productFamily")]
    product_family: Option<String>,
    #[serde(default)]
    attributes: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct Terms {
    #[serde(rename = "OnDemand", default)]
    on_demand: HashMap<String, HashMap<String, Term>>,
}

#[derive(Debug, Deserialize)]
struct Term {
    #[serde(rename = "priceDimensions")]
    price_dimensions: HashMap<String, PriceDimension>,
}

#[derive(Debug, Deserialize)]
struct PriceDimension {
    description: Option<String>,
    #[serde(rename = "beginRange", default)]
    begin_range: Option<String>,
    #[serde(rename = "pricePerUnit")]
    price_per_unit: PricePerUnit,
}

#[derive(Debug, Deserialize)]
struct PricePerUnit {
    #[serde(rename = "USD")]
    usd: Option<String>,
}

async fn fetch_pricing(client: &reqwest::Client, code: &str) -> anyhow::Result<PricingDoc> {
    let url = format!("{BASE}/{code}/current/{REGION}/index.json");
    eprintln!("  fetching {code}");
    let res = client.get(&url).send().await?.error_for_status()?;
    let bytes = res.bytes().await?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Find the first product matching the matcher; pull its OnDemand
/// rate + description.
fn extract_dimension(doc: &PricingDoc, m: &DimensionMatcher) -> Option<(f64, String)> {
    let product = doc.products.values().find(|p| {
        p.product_family.as_deref() == Some(m.product_family)
            && m.attributes
                .iter()
                .all(|(k, v)| p.attributes.get(*k).map(|s| s.as_str()) == Some(*v))
    })?;
    let term = doc.terms.on_demand.get(&product.sku)?.values().next()?;
    // HashMap iteration order is non-deterministic; sort by beginRange
    // so tiered SKUs always pick the lowest-tier (canonical) rate.
    let mut dims: Vec<&PriceDimension> = term.price_dimensions.values().collect();
    dims.sort_by(|a, b| {
        let ar = a
            .begin_range
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let br = b
            .begin_range
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        ar.partial_cmp(&br).unwrap_or(std::cmp::Ordering::Equal)
    });
    let dim = dims.into_iter().next()?;
    let usd = dim.price_per_unit.usd.as_deref()?;
    let rate: f64 = usd.parse().ok()?;
    Some((rate, dim.description.clone().unwrap_or_default()))
}

/// AWS Outbound transfer to Internet has tiered pricing (first 100GB
/// free, then $0.09/GB up to 10TB, etc.). AWSim shows a flat rate; we
/// pull the lowest *paid* tier — i.e. the smallest beginRange whose
/// USD rate is > 0. Fallback $0.09/GB if no SKU matches.
///
/// HashMap iteration order is non-deterministic, so we have to sort
/// every candidate dimension across every matching SKU before picking.
fn extract_data_transfer_out(doc: &PricingDoc) -> f64 {
    let mut tiers: Vec<(f64, f64)> = Vec::new(); // (begin_range_gb, usd)
    for product in doc.products.values() {
        if product.attributes.get("transferType").map(|s| s.as_str()) != Some("AWS Outbound")
            || product.attributes.get("toLocation").map(|s| s.as_str()) != Some("External")
            || product.attributes.get("fromLocation").map(|s| s.as_str()) != Some(REGION_DISPLAY)
        {
            continue;
        }
        let Some(term) = doc
            .terms
            .on_demand
            .get(&product.sku)
            .and_then(|t| t.values().next())
        else {
            continue;
        };
        for dim in term.price_dimensions.values() {
            let Some(usd) = dim
                .price_per_unit
                .usd
                .as_deref()
                .and_then(|s| s.parse::<f64>().ok())
            else {
                continue;
            };
            if usd <= 0.0 {
                continue;
            }
            let begin = dim
                .begin_range
                .as_deref()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            tiers.push((begin, usd));
        }
    }
    tiers.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    tiers.first().map(|(_, usd)| *usd).unwrap_or(0.09)
}

/// Pull the AWS-supplied display name (e.g. "Amazon S3") off any
/// product, falling back to the AWS offer code if none expose one.
fn derive_display_name(doc: &PricingDoc, fallback: &str) -> String {
    for p in doc.products.values() {
        if let Some(name) = p.attributes.get("servicename") {
            return name.clone();
        }
    }
    fallback.to_string()
}

async fn build_service(
    client: &reqwest::Client,
    cfg: &ServiceConfig,
    data_transfer_out_per_gb: f64,
) -> anyhow::Result<ServicePricing> {
    let doc = fetch_pricing(client, cfg.aws_code).await?;
    let display_name = derive_display_name(&doc, cfg.aws_code);
    let pubdate = doc.publication_date.as_deref().unwrap_or("unknown");

    let mut dimensions = Vec::with_capacity(cfg.dimensions.len());
    for dim in cfg.dimensions {
        let (rate, description) = match &dim.matcher {
            Some(m) => extract_dimension(&doc, m).unwrap_or_else(|| {
                eprintln!(
                    "  WARN: no SKU for {}/{:?} — emitting rate 0",
                    m.product_family, m.attributes
                );
                (0.0, dim.fixed_description.to_string())
            }),
            None => (dim.fixed_rate, dim.fixed_description.to_string()),
        };
        dimensions.push(RequestDimension {
            description,
            operations: dim.operations.iter().map(|s| s.to_string()).collect(),
            price_per_request: rate,
        });
    }

    Ok(ServicePricing {
        service: cfg.service.to_string(),
        display_name,
        region: REGION.to_string(),
        currency: "USD".to_string(),
        source: Some(format!(
            "AWS Pricing Bulk JSON ({}, {pubdate})",
            cfg.aws_code
        )),
        request_dimensions: dimensions,
        default_request_rate: Some(cfg.default_request_rate),
        data_transfer_out_per_gb: Some(data_transfer_out_per_gb),
    })
}

fn pricing_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR points at crates/awsim-billing/.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("pricing")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("awsim-billing-refresh/0.1")
        .build()?;

    eprintln!("Fetching outbound transfer rate...");
    let dt_doc = fetch_pricing(&client, "AWSDataTransfer").await?;
    let dt_rate = extract_data_transfer_out(&dt_doc);
    eprintln!("  data_transfer_out_per_gb = ${dt_rate}\n");

    let out_dir = pricing_dir();
    std::fs::create_dir_all(&out_dir)?;

    for cfg in SERVICES {
        eprintln!("Refreshing {}...", cfg.service);
        let slim = build_service(&client, cfg, dt_rate).await?;
        let path = out_dir.join(format!("{}.json", cfg.service));
        // Round-trip through Value so serde_json with `preserve_order`
        // emits stable, human-friendly key ordering.
        let pretty = serde_json::to_string_pretty(&serde_json::to_value(&slim)?)?;
        std::fs::write(&path, format!("{pretty}\n"))?;
        eprintln!(
            "  wrote {} — {} dimensions, ${dt_rate}/GB transfer\n",
            path.file_name().unwrap().to_string_lossy(),
            slim.request_dimensions.len()
        );
    }

    eprintln!("Done. Review the diff with: git diff crates/awsim-billing/pricing/");
    Ok(())
}
