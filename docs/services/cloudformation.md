# CloudFormation

AWS CloudFormation for infrastructure as code — deploying and managing AWS resources through templates.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsQuery` |
| Signing Name | `cloudformation` |
| Persistence | No |

CloudFormation uses the `AwsQuery` protocol: `POST` requests with `Content-Type: application/x-www-form-urlencoded` and an `Action=` parameter.

## Quick Start

Validate a template and create a stack:

```bash
# Validate a template
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/cloudformation/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=ValidateTemplate' \
  --data-urlencode 'Version=2010-05-15' \
  --data-urlencode 'TemplateBody={"AWSTemplateFormatVersion":"2010-09-09","Resources":{"MyBucket":{"Type":"AWS::S3::Bucket"}}}'

# Create a stack
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/cloudformation/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=CreateStack' \
  --data-urlencode 'StackName=my-stack' \
  --data-urlencode 'TemplateBody={"AWSTemplateFormatVersion":"2010-09-09","Resources":{"MyBucket":{"Type":"AWS::S3::Bucket","Properties":{"BucketName":"my-cfn-bucket"}}}}'
```

## Operations

### Stacks
- `CreateStack` — create a new stack from a CloudFormation template
  - Input: `StackName`, `TemplateBody` or `TemplateURL`, `Parameters` (list of `{ParameterKey, ParameterValue}`), `Capabilities` (e.g., `CAPABILITY_IAM`), `Tags`
  - Returns: `StackId` (ARN)
  - Stack transitions through `CREATE_IN_PROGRESS` to `CREATE_COMPLETE` immediately

- `DeleteStack` — delete a stack and mark all its resources as deleted
  - Input: `StackName`

- `UpdateStack` — update a stack with a new template or changed parameters
  - Input: `StackName`, `TemplateBody` or `TemplateURL`, updated `Parameters`
  - Transitions: `UPDATE_IN_PROGRESS` → `UPDATE_COMPLETE`

- `DescribeStacks` — get stack status and outputs
  - Input: optional `StackName` (returns all stacks if omitted)
  - Returns: `Stacks` list with `StackStatus`, `Outputs`, `Parameters`, `StackId`

- `DescribeStackEvents` — retrieve the event history for a stack
  - Input: `StackName`
  - Returns: `StackEvents` list with `ResourceStatus`, `ResourceType`, `Timestamp`

- `DescribeStackResources` — list resources provisioned by a stack
  - Input: `StackName`
  - Returns: `StackResources` with `ResourceType`, `LogicalResourceId`, `PhysicalResourceId`

- `ListStacks` — list stacks with optional status filter
  - Input: optional `StackStatusFilter` (list of statuses)
  - Returns: paginated `StackSummaries`

- `GetTemplate` — retrieve the template body used to create or update a stack
  - Input: `StackName`
  - Returns: `TemplateBody` (JSON string)

- `ValidateTemplate` — validate a template without creating a stack
  - Input: `TemplateBody` or `TemplateURL`
  - Returns: `Parameters`, `Capabilities`, `Description` parsed from the template

### Change Sets
- `CreateChangeSet` — create a change set to preview stack changes before applying
  - Input: `StackName`, `ChangeSetName`, `TemplateBody` or `TemplateURL`, `Parameters`
  - Returns: `Id` (change set ARN)

- `ExecuteChangeSet` — apply a change set to update the stack
  - Input: `StackName`, `ChangeSetName`

- `DeleteChangeSet` — discard a change set without applying it
  - Input: `StackName`, `ChangeSetName`

- `DescribeChangeSet` — get the proposed changes and current status
  - Input: `StackName`, `ChangeSetName`
  - Returns: `Changes` list, `Status` (`CREATE_COMPLETE`)

- `ListChangeSets` — list change sets for a stack
  - Input: `StackName`
  - Returns: `Summaries` list

## Curl Examples

```bash
# 1. Create a stack with parameters
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/cloudformation/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=CreateStack' \
  --data-urlencode 'StackName=app-stack' \
  --data-urlencode 'Parameters.member.1.ParameterKey=Env' \
  --data-urlencode 'Parameters.member.1.ParameterValue=staging' \
  --data-urlencode 'TemplateBody={"AWSTemplateFormatVersion":"2010-09-09","Parameters":{"Env":{"Type":"String"}},"Resources":{"Queue":{"Type":"AWS::SQS::Queue","Properties":{"QueueName":{"Fn::Sub":"myqueue-${Env}"}}}}}'

# 2. Describe stacks
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/cloudformation/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=DescribeStacks' \
  --data-urlencode 'StackName=app-stack'

# 3. List all stacks
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/cloudformation/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=ListStacks'
```

## SDK Example

```typescript
import {
  CloudFormationClient,
  CreateStackCommand,
  DescribeStacksCommand,
  ListStacksCommand,
} from '@aws-sdk/client-cloudformation';

const cfn = new CloudFormationClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

const template = JSON.stringify({
  AWSTemplateFormatVersion: '2010-09-09',
  Resources: {
    MyBucket: {
      Type: 'AWS::S3::Bucket',
      Properties: { BucketName: 'my-cfn-bucket' },
    },
  },
});

// Create stack
const { StackId } = await cfn.send(new CreateStackCommand({
  StackName: 'my-stack',
  TemplateBody: template,
}));

console.log('Stack ID:', StackId);

// Describe the stack
const { Stacks } = await cfn.send(new DescribeStacksCommand({
  StackName: 'my-stack',
}));

console.log('Status:', Stacks?.[0]?.StackStatus); // CREATE_COMPLETE

// List all stacks
const { StackSummaries } = await cfn.send(new ListStacksCommand({}));
console.log('Total stacks:', StackSummaries?.length);
```

## Behavior Notes

- CloudFormation records template metadata and resource definitions but does **not** invoke other AWSim services (e.g., does not create S3 buckets or Lambda functions defined in the template).
- Stack status transitions through `CREATE_IN_PROGRESS` to `CREATE_COMPLETE` immediately on creation.
- Template intrinsic functions (`Ref`, `Fn::Join`, `Fn::Sub`, etc.) are parsed and stored but may not be fully evaluated.
- `DescribeStackResources` returns resource entries with placeholder `PhysicalResourceId` values.
- State is in-memory only and lost on restart.
