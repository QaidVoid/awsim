# Services Overview

AWSim registers 58 AWS services. All services share the same endpoint at `http://localhost:4566`.

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
| EventBridge Pipes | `pipes` | REST-JSON | Yes | 10 | Point-to-point source→target integrations |
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
| EFS | `elasticfilesystem` | REST-JSON | Yes | 19 | Elastic File System (file systems, mount targets, access points) |
| Backup | `backup` | REST-JSON | Yes | 17 | Backup vaults, plans, selections, jobs |
| Application Auto Scaling | `application-autoscaling` | JSON | Yes | 10 | Scalable targets and policies for ECS/Lambda/DynamoDB |
| X-Ray | `xray` | REST-JSON | Yes | 11 | Trace ingest, summaries, service graph |
| Cloud Map | `servicediscovery` | JSON | Yes | 17 | Service discovery — namespaces, services, instances |
| AppConfig | `appconfig` | REST-JSON | Yes | 20 | Feature flags & config delivery (control + data plane) |
| AppConfig Data | `appconfig` (`appconfigdata`) | REST-JSON | Yes | 2 | Runtime polling: StartConfigurationSession + GetLatestConfiguration |
| Glacier | `glacier` | REST-JSON | Yes | 12 | Cold storage: vaults + archives + jobs |
| MQ | `mq` | REST-JSON | Yes | 14 | Amazon MQ brokers, users, configurations |
| MemoryDB | `memorydb` | JSON | Yes | 18 | Redis-compatible clusters, users, ACLs, snapshots |
| QLDB | `qldb` | REST-JSON | Yes | 8 | Ledger metadata (control plane only) |

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
- [EventBridge Pipes](/services/pipes)
- [EFS](/services/efs)
- [Backup](/services/backup)
- [Application Auto Scaling](/services/application-autoscaling)
- [X-Ray](/services/xray)
- [Cloud Map (Service Discovery)](/services/servicediscovery)
- [AppConfig](/services/appconfig)
- [Glacier](/services/glacier)
- [MQ](/services/mq)
- [DocumentDB](/services/docdb)
- [Neptune](/services/neptune)
- [MemoryDB](/services/memorydb)
- [QLDB](/services/qldb)
