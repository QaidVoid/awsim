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
    /// CloudFront and friends bill by edge region rather than
    /// customer region, so their per-region offer files only carry
    /// origin-shield SKUs. Set this to fetch the bulk file with no
    /// region segment instead — the matchers then need to match the
    /// region-prefixed usagetypes (US-Requests-Tier2-HTTPS, etc.).
    use_global_file: bool,
    /// Fallback per-request rate for ops not matched by any dimension.
    default_request_rate: f64,
    /// Optional ingest matcher for byte-billed services (Firehose,
    /// CloudWatch Logs ingest etc.). When set, the rate becomes the
    /// slim file's `data_ingest_per_gb`. AWS publishes ingest rates
    /// in dollars per GB so we emit them as-is.
    ingest_matcher: Option<DimensionMatcher>,
    /// Optional storage matcher for point-in-time billed services
    /// (S3 / DDB / Lambda code). AWS publishes these in GB-Month
    /// units so we emit the rate straight into `storage_per_gb_month`.
    storage_matcher: Option<DimensionMatcher>,
    /// Optional compute matcher for duration-billed services
    /// (Lambda's GB-second axis). AWS publishes these in $/GB-Second,
    /// emitted as `compute_per_gb_second` in the slim file.
    compute_matcher: Option<DimensionMatcher>,
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
        use_global_file: false,
        default_request_rate: 4.0e-7,
        ingest_matcher: None,
        // S3 Standard storage. AWS publishes the rate in $/GB-Mo,
        // first paid tier is $0.023/GB-Mo (50 TB free under tier 0
        // is for the full-organisation free tier; tier 1 is the
        // headline rate).
        storage_matcher: Some(DimensionMatcher {
            product_family: "Storage",
            attributes: &[("usagetype", "TimedStorage-ByteHrs")],
        }),
        compute_matcher: None,
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
        use_global_file: false,
        default_request_rate: 0.0,
        ingest_matcher: None,
        storage_matcher: None,
        // Lambda x86 compute. AWS prices ARM ~20% cheaper but the
        // request event doesn't carry the architecture, so x86 is the
        // safer default (overbills ARM workloads slightly).
        compute_matcher: Some(DimensionMatcher {
            product_family: "Serverless",
            attributes: &[
                ("usagetype", "Lambda-GB-Second"),
                ("group", "AWS-Lambda-Duration"),
            ],
        }),
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
        use_global_file: false,
        default_request_rate: 0.0,
        ingest_matcher: None,
        // DDB Standard table storage at $0.25/GB-Mo (free first 25 GB
        // is the org-wide free tier — extract_dimension's
        // prefer-paid-tier logic picks the right one).
        storage_matcher: Some(DimensionMatcher {
            product_family: "Database Storage",
            attributes: &[("usagetype", "TimedStorage-ByteHrs")],
        }),
        compute_matcher: None,
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
    ServiceConfig {
        service: "sqs",
        aws_code: "AWSQueueService",
        use_global_file: false,
        default_request_rate: 4.0e-7,
        ingest_matcher: None,
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[
            DimensionConfig {
                operations: &[
                    "SendMessage",
                    "SendMessageBatch",
                    "ReceiveMessage",
                    "DeleteMessage",
                    "DeleteMessageBatch",
                    "ChangeMessageVisibility",
                    "ChangeMessageVisibilityBatch",
                    "PurgeQueue",
                    "GetQueueAttributes",
                    "SetQueueAttributes",
                    "GetQueueUrl",
                    "ListQueues",
                    "ListDeadLetterSourceQueues",
                    "AddPermission",
                    "RemovePermission",
                    "TagQueue",
                    "UntagQueue",
                    "ListQueueTags",
                ],
                matcher: Some(DimensionMatcher {
                    product_family: "API Request",
                    attributes: &[("usagetype", "Requests-RBP")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                // FIFO ops aren't named differently from standard ones
                // (the queue ARN ending in `.fifo` is what tells AWS
                // it's FIFO), so we can't bucket per-operation. Keep
                // the dimension visible at AWS's FIFO rate so users
                // see what FIFO would cost; counts stay 0 because
                // RequestEvent doesn't carry queue type.
                operations: &[],
                matcher: Some(DimensionMatcher {
                    product_family: "API Request",
                    attributes: &[("usagetype", "Requests-FIFO-RBP")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &["CreateQueue", "DeleteQueue"],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "sns",
        aws_code: "AmazonSNS",
        use_global_file: false,
        default_request_rate: 5.0e-7,
        ingest_matcher: None,
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[
            DimensionConfig {
                operations: &[
                    "Publish",
                    "PublishBatch",
                    "Subscribe",
                    "Unsubscribe",
                    "ConfirmSubscription",
                    "ListSubscriptions",
                    "ListSubscriptionsByTopic",
                    "GetSubscriptionAttributes",
                    "SetSubscriptionAttributes",
                    "ListTopics",
                    "GetTopicAttributes",
                    "SetTopicAttributes",
                    "AddPermission",
                    "RemovePermission",
                ],
                matcher: Some(DimensionMatcher {
                    product_family: "API Request",
                    attributes: &[
                        ("usagetype", "Requests-Tier1"),
                        ("group", "SNS-Requests-Tier1"),
                    ],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "CreateTopic",
                    "DeleteTopic",
                    "TagResource",
                    "UntagResource",
                    "ListTagsForResource",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "kms",
        aws_code: "awskms",
        use_global_file: false,
        default_request_rate: 3.0e-6,
        ingest_matcher: None,
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[
            DimensionConfig {
                operations: &[
                    "Encrypt",
                    "Decrypt",
                    "ReEncrypt",
                    "GenerateDataKey",
                    "GenerateDataKeyWithoutPlaintext",
                    "GenerateRandom",
                    "GenerateMac",
                    "VerifyMac",
                    "Sign",
                    "Verify",
                    "DeriveSharedSecret",
                ],
                matcher: Some(DimensionMatcher {
                    product_family: "API Request",
                    attributes: &[("group", "awskms-APIRequest-All")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                // Per-key-month is point-in-time and metered separately
                // (not yet wired). Show as $0 here for now so the
                // dashboard doesn't pretend it's been counted.
                operations: &[
                    "CreateKey",
                    "DescribeKey",
                    "ListKeys",
                    "ListAliases",
                    "CreateAlias",
                    "DeleteAlias",
                    "UpdateAlias",
                    "EnableKey",
                    "DisableKey",
                    "ScheduleKeyDeletion",
                    "CancelKeyDeletion",
                    "PutKeyPolicy",
                    "GetKeyPolicy",
                    "ListKeyPolicies",
                    "ListResourceTags",
                    "TagResource",
                    "UntagResource",
                    "EnableKeyRotation",
                    "DisableKeyRotation",
                    "GetKeyRotationStatus",
                    "CreateGrant",
                    "RetireGrant",
                    "RevokeGrant",
                    "ListGrants",
                    "ListRetirableGrants",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "secretsmanager",
        aws_code: "AWSSecretsManager",
        use_global_file: false,
        default_request_rate: 5.0e-6,
        ingest_matcher: None,
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[
            DimensionConfig {
                operations: &[
                    "GetSecretValue",
                    "PutSecretValue",
                    "DescribeSecret",
                    "ListSecrets",
                    "ListSecretVersionIds",
                    "GetResourcePolicy",
                    "PutResourcePolicy",
                    "DeleteResourcePolicy",
                    "ValidateResourcePolicy",
                    "UpdateSecret",
                    "UpdateSecretVersionStage",
                    "RotateSecret",
                    "CancelRotateSecret",
                ],
                matcher: Some(DimensionMatcher {
                    product_family: "API Request",
                    attributes: &[("group", "AWSSecretsManager-APIRequest")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "CreateSecret",
                    "DeleteSecret",
                    "RestoreSecret",
                    "TagResource",
                    "UntagResource",
                    "ReplicateSecretToRegions",
                    "RemoveRegionsFromReplication",
                    "StopReplicationToReplica",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "events",
        aws_code: "AWSEvents",
        use_global_file: false,
        default_request_rate: 0.0,
        ingest_matcher: None,
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[
            DimensionConfig {
                operations: &["PutEvents"],
                matcher: Some(DimensionMatcher {
                    product_family: "EventBridge",
                    // `operation=PutEvents` narrows past the partner /
                    // discovery / cross-account variants that share
                    // the 64K-Chunks usagetype.
                    attributes: &[
                        ("usagetype", "USE1-Event-64K-Chunks"),
                        ("operation", "PutEvents"),
                    ],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "PutRule",
                    "DeleteRule",
                    "DescribeRule",
                    "ListRules",
                    "DisableRule",
                    "EnableRule",
                    "PutTargets",
                    "RemoveTargets",
                    "ListTargetsByRule",
                    "CreateEventBus",
                    "DeleteEventBus",
                    "DescribeEventBus",
                    "ListEventBuses",
                    "PutPermission",
                    "RemovePermission",
                    "TagResource",
                    "UntagResource",
                    "ListTagsForResource",
                    "TestEventPattern",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "apigateway",
        aws_code: "AmazonApiGateway",
        use_global_file: false,
        default_request_rate: 0.0,
        ingest_matcher: None,
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[
            // REST API requests are billed per-call. The
            // `usagetype=USE1-ApiGatewayRequest` SKU's first paid tier
            // is the headline $3.50/M rate.
            DimensionConfig {
                // We can't tell REST from HTTP from the request event
                // alone — both go through the gateway proxy without a
                // protocol tag — so we charge everything at the REST
                // rate. Slight overbill vs HTTP API, but defensible
                // given AWSim doesn't model HTTP-vs-REST distinctly.
                operations: &["ApiGatewayRequest", "Invoke", "ApiInvoke"],
                matcher: Some(DimensionMatcher {
                    product_family: "API Calls",
                    attributes: &[("usagetype", "USE1-ApiGatewayRequest")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "CreateRestApi",
                    "DeleteRestApi",
                    "GetRestApi",
                    "GetRestApis",
                    "UpdateRestApi",
                    "CreateResource",
                    "DeleteResource",
                    "GetResource",
                    "GetResources",
                    "UpdateResource",
                    "PutMethod",
                    "DeleteMethod",
                    "GetMethod",
                    "UpdateMethod",
                    "PutIntegration",
                    "DeleteIntegration",
                    "GetIntegration",
                    "UpdateIntegration",
                    "PutMethodResponse",
                    "PutIntegrationResponse",
                    "CreateDeployment",
                    "GetDeployment",
                    "GetDeployments",
                    "DeleteDeployment",
                    "CreateStage",
                    "GetStage",
                    "GetStages",
                    "UpdateStage",
                    "DeleteStage",
                    "CreateAuthorizer",
                    "GetAuthorizer",
                    "GetAuthorizers",
                    "UpdateAuthorizer",
                    "DeleteAuthorizer",
                    "CreateApiKey",
                    "DeleteApiKey",
                    "GetApiKey",
                    "GetApiKeys",
                    "UpdateApiKey",
                    "CreateUsagePlan",
                    "DeleteUsagePlan",
                    "GetUsagePlan",
                    "GetUsagePlans",
                    "UpdateUsagePlan",
                    "TagResource",
                    "UntagResource",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "states",
        aws_code: "AmazonStates",
        use_global_file: false,
        default_request_rate: 0.0,
        ingest_matcher: None,
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[
            // AWS bills per state transition, not per execution. We
            // can only see StartExecution from the request event, so
            // we charge one transition per execution — a deliberate
            // underbill. Real workflows average 5-10 transitions per
            // run; future work will need to tap into the SFN engine's
            // transition events for accurate metering.
            DimensionConfig {
                operations: &["StartExecution", "StartSyncExecution"],
                matcher: Some(DimensionMatcher {
                    product_family: "AWS Step Functions",
                    attributes: &[("usagetype", "USE1-StateTransition")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "CreateStateMachine",
                    "DeleteStateMachine",
                    "DescribeStateMachine",
                    "ListStateMachines",
                    "UpdateStateMachine",
                    "DescribeExecution",
                    "GetExecutionHistory",
                    "ListExecutions",
                    "StopExecution",
                    "SendTaskSuccess",
                    "SendTaskFailure",
                    "SendTaskHeartbeat",
                    "CreateActivity",
                    "DeleteActivity",
                    "DescribeActivity",
                    "GetActivityTask",
                    "ListActivities",
                    "TagResource",
                    "UntagResource",
                    "ListTagsForResource",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "ses",
        aws_code: "AmazonSES",
        use_global_file: false,
        default_request_rate: 0.0,
        ingest_matcher: None,
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[
            // AWS bills per recipient, not per send. SDK callers
            // typically send to one recipient at a time, so per-call
            // is a close-enough approximation. Multi-recipient sends
            // (Destination.ToAddresses with N entries) underbill by N.
            DimensionConfig {
                operations: &[
                    "SendEmail",
                    "SendRawEmail",
                    "SendBulkEmail",
                    "SendBulkTemplatedEmail",
                    "SendTemplatedEmail",
                ],
                matcher: Some(DimensionMatcher {
                    product_family: "Sending Email",
                    attributes: &[("usagetype", "Recipients"), ("operation", "Send")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "VerifyEmailIdentity",
                    "DeleteIdentity",
                    "GetIdentityVerificationAttributes",
                    "ListIdentities",
                    "VerifyDomainIdentity",
                    "VerifyDomainDkim",
                    "GetSendStatistics",
                    "GetSendQuota",
                    "CreateConfigurationSet",
                    "DeleteConfigurationSet",
                    "DescribeConfigurationSet",
                    "ListConfigurationSets",
                    "CreateTemplate",
                    "DeleteTemplate",
                    "GetTemplate",
                    "ListTemplates",
                    "UpdateTemplate",
                    "TagResource",
                    "UntagResource",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "monitoring",
        aws_code: "AmazonCloudWatch",
        use_global_file: false,
        default_request_rate: 1.0e-5,
        ingest_matcher: None,
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[
            // AWS bills CloudWatch API requests at $0.01 per 1,000.
            // PutMetricData is in the same bucket — its per-metric
            // monthly cost is point-in-time and not yet metered here.
            DimensionConfig {
                operations: &[
                    "GetMetricData",
                    "GetMetricStatistics",
                    "GetMetricWidgetImage",
                    "PutMetricData",
                    "ListMetrics",
                    "DescribeAlarms",
                    "DescribeAlarmsForMetric",
                    "DescribeAlarmHistory",
                    "PutMetricAlarm",
                    "DeleteAlarms",
                    "EnableAlarmActions",
                    "DisableAlarmActions",
                    "SetAlarmState",
                    "PutDashboard",
                    "GetDashboard",
                    "DeleteDashboards",
                    "ListDashboards",
                ],
                matcher: Some(DimensionMatcher {
                    product_family: "API Request",
                    attributes: &[("usagetype", "CW:Requests")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "TagResource",
                    "UntagResource",
                    "ListTagsForResource",
                    "PutAnomalyDetector",
                    "DeleteAnomalyDetector",
                    "DescribeAnomalyDetectors",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "route53",
        aws_code: "AmazonRoute53",
        use_global_file: false,
        default_request_rate: 0.0,
        ingest_matcher: None,
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[
            // The Route53 resolver is metered server-side (DNS resolves
            // never reach the AWSim AWS-API gateway) so this dimension
            // typically stays at count 0. Kept here so the rate is
            // discoverable on the dashboard.
            DimensionConfig {
                operations: &[],
                matcher: Some(DimensionMatcher {
                    product_family: "DNS Query",
                    attributes: &[("usagetype", "USE1-DNS-Queries")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "CreateHostedZone",
                    "DeleteHostedZone",
                    "GetHostedZone",
                    "ListHostedZones",
                    "ListHostedZonesByName",
                    "UpdateHostedZoneComment",
                    "ChangeResourceRecordSets",
                    "ListResourceRecordSets",
                    "GetChange",
                    "CreateHealthCheck",
                    "DeleteHealthCheck",
                    "GetHealthCheck",
                    "ListHealthChecks",
                    "UpdateHealthCheck",
                    "ChangeTagsForResource",
                    "ListTagsForResource",
                    "ListTagsForResources",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "kinesis",
        aws_code: "AmazonKinesis",
        use_global_file: false,
        default_request_rate: 0.0,
        ingest_matcher: None,
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[
            // Provisioned-mode put-payload-units is what AWS actually
            // bills against — one unit per 25KB rounded up. We charge
            // one unit per Put* call, which underbills records >25KB.
            DimensionConfig {
                operations: &["PutRecord", "PutRecords"],
                matcher: Some(DimensionMatcher {
                    product_family: "Kinesis Streams",
                    attributes: &[("usagetype", "PutRequestPayloadUnits")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "CreateStream",
                    "DeleteStream",
                    "DescribeStream",
                    "DescribeStreamSummary",
                    "ListStreams",
                    "GetShardIterator",
                    "GetRecords",
                    "MergeShards",
                    "SplitShard",
                    "IncreaseStreamRetentionPeriod",
                    "DecreaseStreamRetentionPeriod",
                    "RegisterStreamConsumer",
                    "DeregisterStreamConsumer",
                    "ListStreamConsumers",
                    "DescribeStreamConsumer",
                    "AddTagsToStream",
                    "RemoveTagsFromStream",
                    "ListTagsForStream",
                    "EnableEnhancedMonitoring",
                    "DisableEnhancedMonitoring",
                    "UpdateShardCount",
                    "UpdateStreamMode",
                    "StartStreamEncryption",
                    "StopStreamEncryption",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "cloudfront",
        aws_code: "AmazonCloudFront",
        // CloudFront's per-region offer file only contains origin-shield
        // SKUs — the headline request rates are in the global bulk file
        // keyed by edge-region prefix (US-, EU-, AP-, etc.).
        use_global_file: true,
        default_request_rate: 0.0,
        ingest_matcher: None,
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[
            DimensionConfig {
                // CloudFront proxied traffic doesn't typically reach
                // the AWSim API gateway, so this dimension is mostly
                // for informational display.
                operations: &[],
                matcher: Some(DimensionMatcher {
                    product_family: "Request",
                    attributes: &[("usagetype", "US-Requests-Tier2-HTTPS")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "CreateDistribution",
                    "DeleteDistribution",
                    "GetDistribution",
                    "GetDistributionConfig",
                    "ListDistributions",
                    "UpdateDistribution",
                    "CreateInvalidation",
                    "GetInvalidation",
                    "ListInvalidations",
                    "CreateOriginAccessControl",
                    "DeleteOriginAccessControl",
                    "GetOriginAccessControl",
                    "ListOriginAccessControls",
                    "UpdateOriginAccessControl",
                    "TagResource",
                    "UntagResource",
                    "ListTagsForResource",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "firehose",
        aws_code: "AmazonKinesisFirehose",
        use_global_file: false,
        default_request_rate: 0.0,
        // Firehose bills by GB ingested — pulled into
        // `data_ingest_per_gb` and applied against bytes_in.
        ingest_matcher: Some(DimensionMatcher {
            product_family: "Kinesis Firehose",
            attributes: &[
                ("usagetype", "USE1-BilledBytes"),
                ("operation", "PutRecord"),
            ],
        }),
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[
            // Per-request rate is $0 — Firehose bills purely on
            // ingested bytes — but listing PutRecord/PutRecordBatch
            // here ensures the row count shows up in the dashboard.
            DimensionConfig {
                operations: &["PutRecord", "PutRecordBatch"],
                matcher: None,
                fixed_description: "PutRecord / PutRecordBatch",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "CreateDeliveryStream",
                    "DeleteDeliveryStream",
                    "DescribeDeliveryStream",
                    "ListDeliveryStreams",
                    "UpdateDestination",
                    "TagDeliveryStream",
                    "UntagDeliveryStream",
                    "ListTagsForDeliveryStream",
                    "StartDeliveryStreamEncryption",
                    "StopDeliveryStreamEncryption",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "logs",
        // CloudWatch Logs lives inside the AmazonCloudWatch offer
        // (CWL was retconned into CloudWatch's product family even
        // though it has its own service signing name `logs`).
        aws_code: "AmazonCloudWatch",
        use_global_file: false,
        default_request_rate: 0.0,
        // Real cost driver: $0.50/GB ingested via PutLogEvents. Wired
        // up against bytes_in to PutLogEvents calls.
        ingest_matcher: Some(DimensionMatcher {
            product_family: "Data Payload",
            attributes: &[
                ("usagetype", "USE1-DataProcessing-Bytes"),
                ("operation", "PutLogEvents"),
            ],
        }),
        // Archived log retention: $0.03/GB-Mo. Sampled by the same
        // poll loop as S3/Lambda — the BodyStore for the "logs"
        // group gives current bytes.
        storage_matcher: Some(DimensionMatcher {
            product_family: "Storage Snapshot",
            attributes: &[("usagetype", "USE1-TimedStorage-ByteHrs")],
        }),
        compute_matcher: None,
        dimensions: &[
            DimensionConfig {
                operations: &["PutLogEvents"],
                matcher: None,
                fixed_description: "PutLogEvents (billed by GB ingested)",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "CreateLogGroup",
                    "DeleteLogGroup",
                    "DescribeLogGroups",
                    "PutRetentionPolicy",
                    "DeleteRetentionPolicy",
                    "CreateLogStream",
                    "DeleteLogStream",
                    "DescribeLogStreams",
                    "GetLogEvents",
                    "FilterLogEvents",
                    "StartQuery",
                    "GetQueryResults",
                    "StopQuery",
                    "DescribeQueries",
                    "GetLogGroupFields",
                    "GetLogRecord",
                    "PutMetricFilter",
                    "DeleteMetricFilter",
                    "DescribeMetricFilters",
                    "PutSubscriptionFilter",
                    "DeleteSubscriptionFilter",
                    "DescribeSubscriptionFilters",
                    "PutDestination",
                    "DeleteDestination",
                    "PutResourcePolicy",
                    "DeleteResourcePolicy",
                    "DescribeResourcePolicies",
                    "TagResource",
                    "UntagResource",
                    "TagLogGroup",
                    "UntagLogGroup",
                    "ListTagsForResource",
                    "ListTagsLogGroup",
                ],
                matcher: None,
                fixed_description: "Control-plane requests",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "cognito-idp",
        aws_code: "AmazonCognito",
        use_global_file: false,
        // AWS bills Cognito User Pools per active user (MAU), not per
        // API call — and AWSim doesn't track unique principals over
        // time, so the MAU cost stays at zero. The API itself is free
        // at the request level. This config exists to (a) show
        // Cognito as a recognised service in the dashboard when it
        // sees traffic, and (b) record the canonical MAU rate as an
        // informational dimension users can read off.
        default_request_rate: 0.0,
        ingest_matcher: None,
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[
            DimensionConfig {
                operations: &[
                    "SignUp",
                    "ConfirmSignUp",
                    "InitiateAuth",
                    "RespondToAuthChallenge",
                    "AdminInitiateAuth",
                    "AdminRespondToAuthChallenge",
                    "AdminCreateUser",
                    "AdminGetUser",
                    "AdminUpdateUserAttributes",
                    "AdminDeleteUser",
                    "AdminResetUserPassword",
                    "AdminSetUserPassword",
                    "AdminConfirmSignUp",
                    "AdminEnableUser",
                    "AdminDisableUser",
                    "AdminAddUserToGroup",
                    "AdminRemoveUserFromGroup",
                    "AdminListGroupsForUser",
                    "ListUsers",
                    "ListGroups",
                    "GetUser",
                    "UpdateUserAttributes",
                    "ChangePassword",
                    "ConfirmForgotPassword",
                    "ForgotPassword",
                    "ResendConfirmationCode",
                    "RevokeToken",
                    "GlobalSignOut",
                    "AdminUserGlobalSignOut",
                    "CreateUserPool",
                    "DeleteUserPool",
                    "DescribeUserPool",
                    "UpdateUserPool",
                    "ListUserPools",
                    "CreateUserPoolClient",
                    "DeleteUserPoolClient",
                    "DescribeUserPoolClient",
                    "UpdateUserPoolClient",
                    "ListUserPoolClients",
                    "CreateGroup",
                    "DeleteGroup",
                    "GetGroup",
                    "UpdateGroup",
                    "TagResource",
                    "UntagResource",
                    "ListTagsForResource",
                ],
                matcher: None,
                fixed_description: "API requests (free — billed via MAU)",
                fixed_rate: 0.0,
            },
            // Informational MAU dimension. count_for never increments
            // (AWSim doesn't track unique principals over a month);
            // the row is here so users see the canonical $0.0055/MAU
            // rate AWS would charge.
            DimensionConfig {
                operations: &[],
                matcher: Some(DimensionMatcher {
                    product_family: "User Pool MAU",
                    attributes: &[("usagetype", "USE1-CognitoUserPoolsMAU")],
                }),
                fixed_description: "",
                fixed_rate: 0.0,
            },
        ],
    },
    ServiceConfig {
        service: "cognito-identity",
        aws_code: "AmazonCognito",
        use_global_file: false,
        default_request_rate: 0.0,
        ingest_matcher: None,
        storage_matcher: None,
        compute_matcher: None,
        dimensions: &[DimensionConfig {
            operations: &[
                "GetId",
                "GetCredentialsForIdentity",
                "GetOpenIdToken",
                "GetOpenIdTokenForDeveloperIdentity",
                "ListIdentities",
                "DescribeIdentity",
                "DeleteIdentities",
                "MergeDeveloperIdentities",
                "UnlinkIdentity",
                "UnlinkDeveloperIdentity",
                "LookupDeveloperIdentity",
                "CreateIdentityPool",
                "DeleteIdentityPool",
                "DescribeIdentityPool",
                "UpdateIdentityPool",
                "ListIdentityPools",
                "GetIdentityPoolRoles",
                "SetIdentityPoolRoles",
                "TagResource",
                "UntagResource",
                "ListTagsForResource",
            ],
            matcher: None,
            fixed_description: "Identity Pool API (free)",
            fixed_rate: 0.0,
        }],
    },
    ServiceConfig {
        service: "ecr",
        aws_code: "AmazonECR",
        use_global_file: false,
        default_request_rate: 0.0,
        ingest_matcher: None,
        // ECR registry storage: $0.10/GB-Mo for stored container
        // images. Sampled via the body store poll loop.
        storage_matcher: Some(DimensionMatcher {
            product_family: "EC2 Container Registry",
            attributes: &[("usagetype", "TimedStorage-ByteHrs")],
        }),
        compute_matcher: None,
        dimensions: &[
            // Per-request rate is $0 for the ECR API — billing is
            // entirely on stored bytes + cross-region transfer.
            DimensionConfig {
                operations: &[
                    "GetAuthorizationToken",
                    "DescribeRepositories",
                    "DescribeImages",
                    "ListImages",
                    "BatchGetImage",
                    "BatchCheckLayerAvailability",
                    "GetDownloadUrlForLayer",
                ],
                matcher: None,
                fixed_description: "Read API requests",
                fixed_rate: 0.0,
            },
            DimensionConfig {
                operations: &[
                    "CreateRepository",
                    "DeleteRepository",
                    "PutImage",
                    "BatchDeleteImage",
                    "InitiateLayerUpload",
                    "UploadLayerPart",
                    "CompleteLayerUpload",
                    "PutImageScanningConfiguration",
                    "PutLifecyclePolicy",
                    "DeleteLifecyclePolicy",
                    "GetLifecyclePolicy",
                    "PutRepositoryPolicy",
                    "DeleteRepositoryPolicy",
                    "GetRepositoryPolicy",
                    "TagResource",
                    "UntagResource",
                    "ListTagsForResource",
                ],
                matcher: None,
                fixed_description: "Write / control-plane requests",
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

async fn fetch_pricing(
    client: &reqwest::Client,
    code: &str,
    use_global_file: bool,
) -> anyhow::Result<PricingDoc> {
    let url = if use_global_file {
        format!("{BASE}/{code}/current/index.json")
    } else {
        format!("{BASE}/{code}/current/{REGION}/index.json")
    };
    eprintln!(
        "  fetching {code}{}",
        if use_global_file { " (global)" } else { "" }
    );
    let res = client.get(&url).send().await?.error_for_status()?;
    let bytes = res.bytes().await?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Find the first product matching the matcher; pull its OnDemand
/// rate + description. AWS commonly ships a "first N free" dimension
/// alongside the paid tier on the same SKU (SNS, EventBridge, SQS),
/// so we prefer the lowest-beginRange *paid* tier and only fall back
/// to a $0 dimension when every tier on the SKU is genuinely free.
fn extract_dimension(doc: &PricingDoc, m: &DimensionMatcher) -> Option<(f64, String)> {
    let product = doc.products.values().find(|p| {
        p.product_family.as_deref() == Some(m.product_family)
            && m.attributes
                .iter()
                .all(|(k, v)| p.attributes.get(*k).map(|s| s.as_str()) == Some(*v))
    })?;
    let term = doc.terms.on_demand.get(&product.sku)?.values().next()?;
    let mut dims: Vec<&PriceDimension> = term.price_dimensions.values().collect();
    // HashMap iteration order is non-deterministic; sort by beginRange
    // so tiered SKUs always evaluate from the lowest threshold up.
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
    let parse_rate = |d: &PriceDimension| {
        d.price_per_unit
            .usd
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok())
    };
    // Prefer the lowest-tier non-zero rate (the "headline" rate AWS
    // markets after the free tier). Fall back to whatever we have.
    let dim = dims
        .iter()
        .copied()
        .find(|d| parse_rate(d).is_some_and(|v| v > 0.0))
        .or_else(|| dims.into_iter().next())?;
    let rate = parse_rate(dim)?;
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
    let doc = fetch_pricing(client, cfg.aws_code, cfg.use_global_file).await?;
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

    // Pull the per-GB ingest rate when the service config supplied a
    // matcher. Services that bill per-request only leave this null.
    let data_ingest_per_gb =
        cfg.ingest_matcher
            .as_ref()
            .and_then(|m| match extract_dimension(&doc, m) {
                Some((rate, _desc)) => Some(rate),
                None => {
                    eprintln!(
                        "  WARN: no ingest SKU for {}/{:?} — leaving null",
                        m.product_family, m.attributes
                    );
                    None
                }
            });

    // Same logic for the at-rest storage rate (point-in-time billed).
    let storage_per_gb_month =
        cfg.storage_matcher
            .as_ref()
            .and_then(|m| match extract_dimension(&doc, m) {
                Some((rate, _desc)) => Some(rate),
                None => {
                    eprintln!(
                        "  WARN: no storage SKU for {}/{:?} — leaving null",
                        m.product_family, m.attributes
                    );
                    None
                }
            });

    // And the compute rate (Lambda GB-second).
    let compute_per_gb_second = cfg
        .compute_matcher
        .as_ref()
        .and_then(|m| match extract_dimension(&doc, m) {
            Some((rate, _desc)) => Some(rate),
            None => {
                eprintln!(
                    "  WARN: no compute SKU for {}/{:?} — leaving null",
                    m.product_family, m.attributes
                );
                None
            }
        });

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
        data_ingest_per_gb,
        storage_per_gb_month,
        compute_per_gb_second,
        instance_hour_per_instance: None,
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
    let dt_doc = fetch_pricing(&client, "AWSDataTransfer", false).await?;
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
