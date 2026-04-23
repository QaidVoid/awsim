# CloudFormation

AWS CloudFormation for infrastructure as code — deploying and managing AWS resources through templates.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsQuery` |
| Signing Name | `cloudformation` |
| Persistence | No |

## Operations

### Stacks
- `CreateStack` — create a new stack from a CloudFormation template
- `DeleteStack` — delete a stack and all its resources
- `UpdateStack` — update an existing stack with a new template or parameters
- `DescribeStacks` — get stack status and outputs, optionally filtered by stack name
- `DescribeStackEvents` — retrieve the event history for a stack
- `DescribeStackResources` — list resources provisioned by a stack
- `ListStacks` — list stacks with optional status filter
- `GetTemplate` — retrieve the template body for a stack
- `ValidateTemplate` — validate a template without creating a stack

### Change Sets
- `CreateChangeSet` — create a change set to preview stack changes
- `ExecuteChangeSet` — apply a change set to update a stack
- `DeleteChangeSet` — delete a change set without applying it
- `DescribeChangeSet` — get the status and proposed changes in a change set
- `ListChangeSets` — list change sets for a stack

## Example

```bash
# Validate a template
aws --endpoint-url http://localhost:4567 \
  cloudformation validate-template \
  --template-body '{"AWSTemplateFormatVersion":"2010-09-09","Resources":{"MyBucket":{"Type":"AWS::S3::Bucket"}}}'

# Create a stack
aws --endpoint-url http://localhost:4567 \
  cloudformation create-stack \
  --stack-name my-stack \
  --template-body '{"AWSTemplateFormatVersion":"2010-09-09","Resources":{"MyBucket":{"Type":"AWS::S3::Bucket","Properties":{"BucketName":"my-cfn-bucket"}}}}'

# Describe the stack
aws --endpoint-url http://localhost:4567 \
  cloudformation describe-stacks \
  --stack-name my-stack

# List all stacks
aws --endpoint-url http://localhost:4567 \
  cloudformation list-stacks
```

## Notes

- CloudFormation in AWSim processes templates and records resource metadata but does not invoke other AWSim services to create the actual resources (e.g., S3 buckets, Lambda functions).
- Stack status transitions through `CREATE_IN_PROGRESS` to `CREATE_COMPLETE` immediately.
- CloudFormation uses the `AwsQuery` protocol (form-encoded POST with `Action=` parameter).
- Template intrinsic functions (`Ref`, `Fn::Join`, etc.) are parsed but may not be fully evaluated.
