use std::collections::HashSet;

use awsim_conformance::smithy::SmithyModel;

pub mod common;

pub mod acm;
pub mod appsync;
pub mod athena;
pub mod batch;
pub mod bedrock;
pub mod cloudformation;
pub mod cloudfront;
pub mod cloudtrail;
pub mod cloudwatch_logs;
pub mod cognito_identity;
pub mod cognito_idp;
pub mod datasync;
pub mod dynamodb;
pub mod ec2;
pub mod ecr;
pub mod ecs;
pub mod eks;
pub mod elb;
pub mod eventbridge;
pub mod firehose;
pub mod glue;
pub mod iam;
pub mod kinesis;
pub mod kms;
pub mod lambda;
pub mod organizations;
pub mod polly;
pub mod rds;
pub mod route53;
pub mod s3;
pub mod scheduler;
pub mod secretsmanager;
pub mod sns;
pub mod sqs;
pub mod ssm;
pub mod sso_admin;
pub mod stepfunctions;
pub mod sts;
pub mod waf;

pub use common::{OpResult, ServiceResult};

pub async fn test_service(
    endpoint: &str,
    service_name: &str,
    model: &SmithyModel,
    verbose: bool,
) -> ServiceResult {
    let op_results = match service_name {
        "sts" => sts::test_sts(endpoint, verbose).await,
        "dynamodb" => dynamodb::test_dynamodb(endpoint, verbose).await,
        "s3" => s3::test_s3(endpoint, verbose).await,
        "sqs" => sqs::test_sqs(endpoint, verbose).await,
        "sns" => sns::test_sns(endpoint, verbose).await,
        "iam" => iam::test_iam(endpoint, verbose).await,
        "kms" => kms::test_kms(endpoint, verbose).await,
        "secretsmanager" => secretsmanager::test_secretsmanager(endpoint, verbose).await,
        "ssm" => ssm::test_ssm(endpoint, verbose).await,
        "lambda" => lambda::test_lambda(endpoint, verbose).await,
        "kinesis" => kinesis::test_kinesis(endpoint, verbose).await,
        "cognito-idp" => cognito_idp::test_cognito_idp(endpoint, verbose).await,
        "cognito-identity" => cognito_identity::test_cognito_identity(endpoint, verbose).await,
        "ecs" => ecs::test_ecs(endpoint, verbose).await,
        "ecr" => ecr::test_ecr(endpoint, verbose).await,
        "eventbridge" => eventbridge::test_eventbridge(endpoint, verbose).await,
        "stepfunctions" => stepfunctions::test_stepfunctions(endpoint, verbose).await,
        "cloudwatch-logs" => cloudwatch_logs::test_cloudwatch_logs(endpoint, verbose).await,
        "ec2" => ec2::test_ec2(endpoint, verbose).await,
        "cloudformation" => cloudformation::test_cloudformation(endpoint, verbose).await,
        "rds" => rds::test_rds(endpoint, verbose).await,
        "route53" => route53::test_route53(endpoint, verbose).await,
        "cloudfront" => cloudfront::test_cloudfront(endpoint, verbose).await,
        "elasticloadbalancingv2" => elb::test_elb(endpoint, verbose).await,
        "acm" => acm::test_acm(endpoint, verbose).await,
        "wafv2" => waf::test_waf(endpoint, verbose).await,
        "scheduler" => scheduler::test_scheduler(endpoint, verbose).await,
        "appsync" => appsync::test_appsync(endpoint, verbose).await,
        "glue" => glue::test_glue(endpoint, verbose).await,
        "athena" => athena::test_athena(endpoint, verbose).await,
        "bedrock" => bedrock::test_bedrock(endpoint, verbose).await,
        "organizations" => organizations::test_organizations(endpoint, verbose).await,
        "cloudtrail" => cloudtrail::test_cloudtrail(endpoint, verbose).await,
        "eks" => eks::test_eks(endpoint, verbose).await,
        "firehose" => firehose::test_firehose(endpoint, verbose).await,
        "batch" => batch::test_batch(endpoint, verbose).await,
        "datasync" => datasync::test_datasync(endpoint, verbose).await,
        "polly" => polly::test_polly(endpoint, verbose).await,
        "sso-admin" => sso_admin::test_sso_admin(endpoint, verbose).await,
        _ => {
            return ServiceResult {
                service: service_name.to_string(),
                total: model.operations().len(),
                implemented: 0,
                passed: 0,
                failed: 0,
                results: Vec::new(),
            };
        }
    };

    let smithy_ops: HashSet<String> = model.operation_names();
    let tested_ops: HashSet<String> = op_results.iter().map(|r| r.op_name().to_string()).collect();

    let total = smithy_ops.len();
    let implemented = tested_ops.intersection(&smithy_ops).count();
    let passed = op_results.iter().filter(|r| r.is_pass()).count();
    let failed = op_results.iter().filter(|r| r.is_fail()).count();

    if verbose {
        let mut missing: Vec<&String> = smithy_ops
            .iter()
            .filter(|op| !tested_ops.contains(*op))
            .collect();
        missing.sort();
        if !missing.is_empty() {
            println!(
                "  [{}] untested Smithy operations: {}",
                service_name,
                missing
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    ServiceResult {
        service: service_name.to_string(),
        total,
        implemented,
        passed,
        failed,
        results: op_results,
    }
}
