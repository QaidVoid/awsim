# ECR

Amazon Elastic Container Registry for storing and managing Docker container images.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `ecr` |
| Target Prefix | `AmazonEC2ContainerRegistry_V20150921` |
| Persistence | Yes (with `--data-dir`) |

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

### Lifecycle Policies
- `PutLifecyclePolicy` — set the lifecycle policy for a repository
  - Input: `repositoryName`, `lifecyclePolicyText` (JSON policy string)

- `GetLifecyclePolicy` — retrieve the lifecycle policy for a repository
  - Input: `repositoryName`
  - Returns: `lifecyclePolicyText`, `lastEvaluatedAt`

- `DeleteLifecyclePolicy` — remove the lifecycle policy from a repository
  - Input: `repositoryName`

### Repository Policies
- `SetRepositoryPolicy` — set an IAM-style access policy on a repository
  - Input: `repositoryName`, `policyText` (JSON policy string), optional `force`

- `GetRepositoryPolicy` — retrieve the access policy for a repository
  - Input: `repositoryName`
  - Returns: `policyText`

- `DeleteRepositoryPolicy` — remove the access policy from a repository
  - Input: `repositoryName`

### Image Scanning
- `StartImageScan` — initiate a vulnerability scan for an image (immediately returns COMPLETE)
  - Input: `repositoryName`, `imageId` (`{imageTag}` or `{imageDigest}`)

- `DescribeImageScanFindings` — retrieve scan results for an image (stub: returns no findings)
  - Input: `repositoryName`, `imageId`
  - Returns: `imageScanFindings` with empty `findings` list

### Layer Operations
- `GetDownloadUrlForLayer` — return a working download URL for a stored layer
  - Input: `repositoryName`, `layerDigest`
  - Returns: `downloadUrl` of the form `http://localhost:{port}/v2/{repo}/blobs/{digest}`, `layerDigest`
  - Errors with `LayersNotFoundException` if the layer is not present

- `BatchCheckLayerAvailability` — check if specific layers exist in the repository
  - Input: `repositoryName`, `layerDigests`
  - Returns: `layers` list with `layerAvailability: "AVAILABLE"` and the real `layerSize`/`mediaType`; missing digests appear in `failures` with `failureCode: "MissingLayerDigest"`

- `InitiateLayerUpload` — start a layer upload session
  - Input: `repositoryName`
  - Returns: `uploadId`, `lastByteReceived: 0`

- `UploadLayerPart` — upload a part of a layer
  - Input: `repositoryName`, `uploadId`, `partFirstByte`, `partLastByte`, `layerPartBlob`
  - Returns: `uploadId`, `lastByteReceived`

- `CompleteLayerUpload` — finalize a layer upload and compute its digest
  - Input: `repositoryName`, `uploadId`, `layerDigests` (client-expected digests)
  - Returns: `uploadId`, `layerDigest` (SHA-256 of uploaded data)

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
- Image manifests are stored as JSON strings.
- Completed layer uploads are stored under the repository: `BatchCheckLayerAvailability` reports real availability and `GetDownloadUrlForLayer` returns a working URL pointing to the local `/v2/{repo}/blobs/{digest}` endpoint.
- The `/v2/{repo}/blobs/{digest}` endpoint streams stored layer bytes with `Content-Type: application/vnd.docker.image.rootfs.diff.tar.gzip` and `Docker-Content-Digest: <digest>` headers.
- `BatchDeleteImage` parses each removed image manifest and best-effort cleans up its referenced layers (memory + on-disk blobs); `DeleteRepository` best-effort removes the entire on-disk layer bucket.
- With `--data-dir`, completed layer bodies are persisted under `{data_dir}/ecr/{repository}/{digest}` and metadata (digest, size, media type) rides in the `ecr.json` snapshot. In-progress upload buffers are kept in memory only.
- `repositoryUri` follows the real AWS format: `{accountId}.dkr.ecr.{region}.amazonaws.com/{name}`.
