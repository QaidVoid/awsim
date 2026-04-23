# ECR

Amazon Elastic Container Registry for storing and managing Docker container images.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `ecr` |
| Target Prefix | `AmazonEC2ContainerRegistry_V20150921` |
| Persistence | No |

## Quick Start

Create a repository, get a login token, and list images:

```bash
# Create a repository
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.CreateRepository" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ecr/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"repositoryName":"my-app","imageScanningConfiguration":{"scanOnPush":false},"imageTagMutability":"MUTABLE"}'

# Get authorization token for docker login
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.GetAuthorizationToken" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ecr/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{}'
```

## Operations

### Repositories
- `CreateRepository` — create a new image repository
  - Input: `repositoryName` (required), `imageTagMutability` (`MUTABLE` or `IMMUTABLE`), `imageScanningConfiguration` (`{scanOnPush: bool}`), `encryptionConfiguration`, `tags`
  - Returns: `repository` with `repositoryArn`, `repositoryUri` (e.g., `000000000000.dkr.ecr.us-east-1.amazonaws.com/my-app`), `createdAt`

- `DeleteRepository` — delete a repository and all its images
  - Input: `repositoryName`, optional `force` (boolean, required if repository contains images)

- `DescribeRepositories` — list repositories with optional filter
  - Input: optional `repositoryNames` (list), `maxResults`, `nextToken`
  - Returns: paginated `repositories` list

### Authorization
- `GetAuthorizationToken` — get a temporary Docker login token for the registry
  - Input: optional `registryIds` (list)
  - Returns: `authorizationData` list, each with `authorizationToken` (base64-encoded `AWS:password`), `expiresAt`, `proxyEndpoint`
  - Decode the token and use as Docker credentials: `echo $TOKEN | base64 -d` gives `AWS:password`

### Images
- `PutImage` — push an image manifest to a repository
  - Input: `repositoryName`, `imageManifest` (JSON string), optional `imageTag`, `imageDigest`
  - Returns: `image` with `imageId` containing `imageDigest` and `imageTag`

- `BatchGetImage` — retrieve image manifests by tag or digest
  - Input: `repositoryName`, `imageIds` (list of `{imageTag}` or `{imageDigest}`)
  - Returns: `images` list with manifests, `failures` for not-found images

- `BatchDeleteImage` — delete one or more images by tag or digest
  - Input: `repositoryName`, `imageIds` (list of `{imageTag}` or `{imageDigest}`)
  - Returns: `imageIds` (deleted) and `failures`

- `ListImages` — list image IDs (tags and digests) in a repository
  - Input: `repositoryName`, optional `filter` (`{tagStatus: "TAGGED"/"UNTAGGED"}`), `maxResults`, `nextToken`
  - Returns: paginated `imageIds` list

- `DescribeImages` — get detailed image metadata including size and push date
  - Input: `repositoryName`, optional `imageIds`, `maxResults`, `nextToken`
  - Returns: `imageDetails` list with `imageSizeInBytes`, `imagePushedAt`, `imageTags`, `imageDigest`

### Tags
- `TagResource` — add tags to a repository (by ARN)
- `UntagResource` — remove tags from a repository
- `ListTagsForResource` — list tags on a repository

## Curl Examples

```bash
# 1. Create a repository with scanning enabled
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.CreateRepository" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ecr/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"repositoryName":"backend","tags":[{"Key":"team","Value":"platform"}]}'

# 2. List all repositories
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.DescribeRepositories" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ecr/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{}'

# 3. Delete image by tag
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.BatchDeleteImage" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ecr/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"repositoryName":"my-app","imageIds":[{"imageTag":"latest"},{"imageTag":"v1.0.0"}]}'
```

## SDK Example

```typescript
import {
  ECRClient,
  CreateRepositoryCommand,
  GetAuthorizationTokenCommand,
  DescribeRepositoriesCommand,
  ListImagesCommand,
} from '@aws-sdk/client-ecr';

const ecr = new ECRClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create repository
const { repository } = await ecr.send(new CreateRepositoryCommand({
  repositoryName: 'my-app',
  imageTagMutability: 'MUTABLE',
}));

console.log('Registry URI:', repository?.repositoryUri);
// e.g., 000000000000.dkr.ecr.us-east-1.localhost.localstack.cloud:4566/my-app

// Get login token
const { authorizationData } = await ecr.send(new GetAuthorizationTokenCommand({}));
const tokenData = authorizationData?.[0];
const [username, password] = Buffer.from(tokenData!.authorizationToken!, 'base64')
  .toString()
  .split(':');

console.log('Docker login user:', username); // AWS
console.log('Registry endpoint:', tokenData?.proxyEndpoint);

// List repositories
const { repositories } = await ecr.send(new DescribeRepositoriesCommand({}));
console.log('Repositories:', repositories?.map(r => r.repositoryName));
```

## Behavior Notes

- The authorization token returned by `GetAuthorizationToken` is a valid base64-encoded string in `AWS:token` format but does not perform real authentication against a Docker registry.
- Image manifests are stored as JSON strings; no image layer validation, decompression, or content-addressable storage occurs.
- ECR does not integrate with a real Docker registry — `docker push/pull` commands require a real registry endpoint.
- `repositoryUri` follows the real AWS format: `{accountId}.dkr.ecr.{region}.amazonaws.com/{name}`.
- State is in-memory only and lost on restart.
