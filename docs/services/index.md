# Services Overview

AWSim registers 60+ AWS services. All services share the same endpoint at `http://localhost:4566`.

The service is selected automatically from the `X-Amz-Target` header (for JSON/Query protocols) or the URL path (for REST protocols).

## CBOR

AWSim accepts CBOR request bodies as well as JSON, in both dialects AWS uses:

| Dialect | How it arrives | Who sends it |
|---|---|---|
| Legacy AWS CBOR | `Content-Type: application/x-amz-cbor-1.1` plus `X-Amz-Target` | AWS SDK for Java v1, which defaults to CBOR for DynamoDB and Kinesis |
| Smithy rpcv2Cbor | `smithy-protocol: rpc-v2-cbor`, routed at `/service/{Service}/operation/{Operation}` | Newer SDKs negotiating the Smithy protocol |

Detection is per request and driven entirely by headers, so nothing changes for existing JSON clients. Responses are encoded in the same format as the request, with errors carrying the same `__type` code and `x-amzn-RequestId` header the JSON path returns.

Binary values round-trip correctly: a DynamoDB `B` or `BS` attribute arrives as a CBOR byte string and goes back out as one, rather than leaking the base64 form that the JSON protocols use internally.

## Service Table

| Service | Signing Name | Protocol | Persistent | Operations | Description |
|---------|-------------|----------|-----------|-----------|-------------|
| ACM | `acm` | JSON | Yes | — | Certificate Manager |
| API Gateway | `execute-api` | REST-JSON | No | — | REST API management and proxy (v1 + v2) |
| AppConfig | `appconfig` | REST-JSON | Yes | 20 | Feature flags & config delivery (control + data plane) |
| AppConfig Data | `appconfig` (`appconfigdata`) | REST-JSON | Yes | 2 | Runtime polling: StartConfigurationSession + GetLatestConfiguration |
| Application Auto Scaling | `application-autoscaling` | JSON | Yes | 10 | Scalable targets and policies for ECS/Lambda/DynamoDB |
| AppSync | `appsync` | REST-JSON | Yes | — | GraphQL API |
| Athena | `athena` | JSON | Yes | — | SQL query service |
| Backup | `backup` | REST-JSON | Yes | 17 | Backup vaults, plans, selections, jobs |
| Batch | `batch` | REST-JSON | Yes | — | Batch compute jobs |
| Bedrock | `bedrock` | REST-JSON | No | — | Foundation model management |
| Bedrock Runtime | `bedrock-runtime` | REST-JSON | No | — | Foundation model invocation |
| Cloud Map | `servicediscovery` | JSON | Yes | 17 | Service discovery: namespaces, services, instances |
| CloudFormation | `cloudformation` | Query | Yes | — | Infrastructure as code |
| CloudFront | `cloudfront` | REST-XML | Yes | — | CDN distributions |
| CloudTrail | `cloudtrail` | JSON | Yes | — | API audit log |
| CloudWatch Logs | `logs` | JSON | Yes | — | Log groups and streams |
| CloudWatch Metrics | `monitoring` | Query | No | — | Metrics and alarms |
| Cognito Identity Pools | `cognito-identity` | JSON | Yes | — | Federated identity |
| Cognito User Pools | `cognito-idp` | JSON | Yes | — | User authentication |
| Comprehend | `comprehend` | JSON | No | — | Natural language processing |
| DataSync | `datasync` | JSON | Yes | — | Data transfer service |
| DynamoDB | `dynamodb` | JSON | Yes | 57 | Key-value / document store |
| EC2 | `ec2` | EC2-Query | Yes | — | Virtual machines (partial) |
| ECR | `ecr` | JSON | Yes | — | Container registry |
| ECS | `ecs` | JSON | Yes | — | Container service |
| EFS | `elasticfilesystem` | REST-JSON | Yes | 19 | Elastic File System (file systems, mount targets, access points) |
| EKS | `eks` | REST-JSON | Yes | — | Kubernetes control plane |
| ELB | `elasticloadbalancing` | Query | Yes | — | Load balancers |
| EventBridge | `events` | JSON | Yes | — | Event routing |
| EventBridge Pipes | `pipes` | REST-JSON | Yes | 10 | Point-to-point source -> target integrations |
| EventBridge Scheduler | `scheduler` | REST-JSON | Yes | — | Scheduled tasks |
| Firehose | `firehose` | JSON | Yes | — | Streaming data delivery |
| Glacier | `glacier` | REST-JSON | Yes | 12 | Cold storage: vaults + archives + jobs |
| Glue | `glue` | JSON | Yes | — | Data catalog |
| IAM | `iam` | Query | Yes | — | Identity and Access Management |
| Identity Store | `identitystore` | JSON | Yes | 16 | Users, groups, group memberships (paired with SSO Admin) |
| Kendra | `kendra` | JSON | No | — | Enterprise search |
| Kinesis | `kinesis` | JSON | No | — | Data streaming |
| KMS | `kms` | JSON | Yes | 28 | Key Management Service |
| Lambda | `lambda` | REST-JSON | Yes | — | Serverless function execution |
| MemoryDB | `memorydb` | JSON | Yes | 18 | Redis-compatible clusters, users, ACLs, snapshots |
| MQ | `mq` | REST-JSON | Yes | 14 | Amazon MQ brokers, users, configurations |
| Organizations | `organizations` | JSON | Yes | — | Account / OU / SCP management |
| Pinpoint | `mobiletargeting` | REST-JSON | Yes | 15 | Apps, endpoints, segments, campaigns (no real delivery) |
| Polly | `polly` | REST-JSON | Yes | — | Text-to-speech |
| QLDB | `qldb` | REST-JSON | Yes | 8 | Ledger metadata (control plane only) |
| RDS | `rds` | Query | Yes | — | Relational database metadata |
| Resource Groups Tagging API | `tagging` | JSON | Yes | 8 | Cross-service resource discovery by tags |
| Route 53 | `route53` | REST-XML | Yes | — | DNS management |
| S3 | `s3` | REST-XML | Yes* | 44 | Object storage |
| Secrets Manager | `secretsmanager` | JSON | Yes | — | Secret storage |
| SES | `ses` | REST-JSON (v2) + Query (classic) | Yes | — | Simple Email Service, both APIs |
| SNS | `sns` | Query | Yes | 21 | Simple Notification Service |
| SQS | `sqs` | Query | Yes | 17 | Simple Queue Service |
| SSM | `ssm` | JSON | Yes | — | Parameter Store and Systems Manager |
| SSO Admin | `sso` | JSON | Yes | — | IAM Identity Center admin |
| Step Functions | `states` | JSON | Yes | — | State machine orchestration |
| STS | `sts` | Query | No | — | Security Token Service |
| Transfer Family | `transfer` | JSON | Yes | 13 | SFTP/FTP servers, users, SSH keys (no actual listener) |
| WAF | `wafv2` | JSON | Yes | — | Web Application Firewall |
| X-Ray | `xray` | REST-JSON | Yes | 11 | Trace ingest, summaries, service graph |

*S3 persists bucket and object metadata but not object data bytes.

## OpenSearch

In addition to the AWS services, AWSim mounts an Elasticsearch-compatible REST API at `/opensearch/`. This is not a standard AWS service endpoint — see [OpenSearch](/guide/opensearch) for details.

## Protocol Notes

- **REST-XML** — URL-based routing, XML request/response bodies (S3, Route 53, CloudFront)
- **REST-JSON** — URL-based routing, JSON request/response bodies (Lambda, API Gateway, AppSync)
- **JSON** — `X-Amz-Target` header routing, JSON bodies (DynamoDB, KMS, CloudWatch Logs)
- **Query** — form-encoded request body with `Action=` parameter (SQS, SNS, IAM, EC2)

## Detailed Service Pages

For operations lists, SDK examples, and limitations, see:

- [ACM](/services/acm)
- [API Gateway](/services/apigateway)
- [AppConfig](/services/appconfig)
- [Application Auto Scaling](/services/application-autoscaling)
- [AppSync](/services/appsync)
- [Athena](/services/athena)
- [Backup](/services/backup)
- [Bedrock](/services/bedrock)
- [Cloud Map (Service Discovery)](/services/servicediscovery)
- [CloudFormation](/services/cloudformation)
- [CloudFront](/services/cloudfront)
- [CloudWatch Logs](/services/cloudwatch-logs)
- [CloudWatch Metrics](/services/cloudwatch-metrics)
- [Cognito](/services/cognito)
- [Comprehend](/services/comprehend)
- [DocumentDB](/services/docdb)
- [DynamoDB](/services/dynamodb)
- [EC2](/services/ec2)
- [ECR](/services/ecr)
- [ECS](/services/ecs)
- [EFS](/services/efs)
- [ELB](/services/elb)
- [EventBridge](/services/eventbridge)
- [EventBridge Pipes](/services/pipes)
- [EventBridge Scheduler](/services/scheduler)
- [Glacier](/services/glacier)
- [Glue](/services/glue)
- [IAM & STS](/services/iam)
- [Identity Store](/services/identitystore)
- [Kendra](/services/kendra)
- [Kinesis](/services/kinesis)
- [KMS](/services/kms)
- [Lambda](/services/lambda)
- [MemoryDB](/services/memorydb)
- [MQ](/services/mq)
- [Neptune](/services/neptune)
- [OpenSearch](/services/opensearch)
- [Pinpoint](/services/pinpoint)
- [QLDB](/services/qldb)
- [RDS](/services/rds)
- [Resource Groups Tagging API](/services/resourcegroupstagging)
- [Route 53](/services/route53)
- [S3](/services/s3)
- [Secrets Manager](/services/secretsmanager)
- [SES](/services/ses)
- [SNS](/services/sns)
- [SQS](/services/sqs)
- [SSM](/services/ssm)
- [Step Functions](/services/stepfunctions)
- [Transfer Family](/services/transfer)
- [WAF](/services/waf)
- [X-Ray](/services/xray)
