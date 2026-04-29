# Services Overview

AWSim registers 46 AWS services. All services share the same endpoint at `http://localhost:4566`.

The service is selected automatically from the `X-Amz-Target` header (for JSON/Query protocols) or the URL path (for REST protocols).

## Service Table

| Service | Signing Name | Protocol | Persistent | Operations | Description |
|---------|-------------|----------|-----------|-----------|-------------|
| S3 | `s3` | REST-XML | Yes* | 44 | Object storage |
| DynamoDB | `dynamodb` | JSON | Yes | 57 | Key-value / document store |
| SQS | `sqs` | Query | Yes | 17 | Simple Queue Service |
| SNS | `sns` | Query | Yes | 21 | Simple Notification Service |
| IAM | `iam` | Query | Yes | — | Identity and Access Management |
| STS | `sts` | Query | No | — | Security Token Service |
| Lambda | `lambda` | REST-JSON | No | — | Serverless function execution |
| API Gateway | `execute-api` | REST-JSON | No | — | REST API management and proxy |
| EventBridge | `events` | JSON | No | — | Event routing |
| CloudWatch Logs | `logs` | JSON | No | — | Log groups and streams |
| KMS | `kms` | JSON | No | 28 | Key Management Service |
| Secrets Manager | `secretsmanager` | JSON | No | — | Secret storage |
| SSM | `ssm` | JSON | No | — | Parameter Store and Systems Manager |
| Step Functions | `states` | JSON | No | — | State machine orchestration |
| Kinesis | `kinesis` | JSON | No | — | Data streaming |
| SES | `ses` | Query | No | — | Simple Email Service |
| Cognito User Pools | `cognito-idp` | JSON | Yes | — | User authentication |
| Cognito Identity Pools | `cognito-identity` | JSON | Yes | — | Federated identity |
| ECR | `ecr` | JSON | No | — | Container registry |
| ECS | `ecs` | JSON | No | — | Container service |
| EC2 | `ec2` | Query | No | — | Virtual machines (partial) |
| RDS | `rds` | Query | Yes | — | Relational database metadata |
| AppSync | `appsync` | REST-JSON | No | — | GraphQL API |
| Bedrock | `bedrock` | REST-JSON | No | — | Foundation model management |
| Bedrock Runtime | `bedrock-runtime` | REST-JSON | No | — | Foundation model invocation |
| CloudFormation | `cloudformation` | Query | No | — | Infrastructure as code |
| Route 53 | `route53` | REST-XML | No | — | DNS management |
| CloudWatch Metrics | `monitoring` | Query | No | — | Metrics and alarms |
| Athena | `athena` | JSON | No | — | SQL query service |
| Glue | `glue` | JSON | No | — | Data catalog |
| ELB | `elasticloadbalancing` | Query | No | — | Load balancers |
| CloudFront | `cloudfront` | REST-XML | No | — | CDN distributions |
| ACM | `acm` | JSON | Yes | — | Certificate Manager |
| WAF | `wafv2` | JSON | Yes | — | Web Application Firewall |
| EventBridge Scheduler | `scheduler` | REST-JSON | Yes | — | Scheduled tasks |
| Comprehend | `comprehend` | JSON | No | — | Natural language processing |
| Kendra | `kendra` | JSON | No | — | Enterprise search |
| Resource Groups Tagging API | `tagging` | JSON | Yes | 8 | Cross-service resource discovery by tags |
| Organizations | `organizations` | JSON | No | — | Account / OU / SCP management |
| CloudTrail | `cloudtrail` | JSON | No | — | API audit log |
| Firehose | `firehose` | JSON | No | — | Streaming data delivery |
| EKS | `eks` | REST-JSON | No | — | Kubernetes control plane |
| Batch | `batch` | REST-JSON | No | — | Batch compute jobs |
| SSO Admin | `sso` | JSON | No | — | IAM Identity Center admin |
| DataSync | `datasync` | JSON | No | — | Data transfer service |
| Polly | `polly` | REST-JSON | No | — | Text-to-speech |

*S3 persists bucket and object metadata but not object data bytes.

## OpenSearch

In addition to the 37 AWS services, AWSim mounts an Elasticsearch-compatible REST API at `/opensearch/`. This is not a standard AWS service endpoint — see [OpenSearch](/guide/opensearch) for details.

## Protocol Notes

- **REST-XML** — URL-based routing, XML request/response bodies (S3, Route 53, CloudFront)
- **REST-JSON** — URL-based routing, JSON request/response bodies (Lambda, API Gateway, AppSync)
- **JSON** — `X-Amz-Target` header routing, JSON bodies (DynamoDB, KMS, CloudWatch Logs)
- **Query** — form-encoded request body with `Action=` parameter (SQS, SNS, IAM, EC2)

## Detailed Service Pages

For operations lists, SDK examples, and limitations, see:

- [S3](/services/s3)
- [DynamoDB](/services/dynamodb)
- [SQS](/services/sqs)
- [SNS](/services/sns)
- [Lambda](/services/lambda)
- [Cognito](/services/cognito)
- [IAM & STS](/services/iam)
- [Resource Groups Tagging API](/services/resourcegroupstagging)
