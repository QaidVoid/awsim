# ECR

Amazon Elastic Container Registry for storing and managing Docker container images.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `ecr` |
| Persistence | No |

## Operations

### Repositories
- `CreateRepository` — create a new image repository
- `DeleteRepository` — delete a repository and all its images
- `DescribeRepositories` — list repositories with optional name filter

### Authorization
- `GetAuthorizationToken` — get a temporary Docker login token for the registry

### Images
- `PutImage` — push an image manifest to a repository
- `BatchGetImage` — retrieve image manifests by tag or digest
- `BatchDeleteImage` — delete one or more images by tag or digest
- `ListImages` — list image IDs in a repository
- `DescribeImages` — get detailed image metadata including size and push date

### Tags
- `TagResource` — add tags to a repository
- `UntagResource` — remove tags from a repository
- `ListTagsForResource` — list tags on a repository

## Example

```bash
# Create a repository
aws --endpoint-url http://localhost:4567 \
  ecr create-repository \
  --repository-name my-app

# Get login token for Docker
aws --endpoint-url http://localhost:4567 \
  ecr get-authorization-token

# List repositories
aws --endpoint-url http://localhost:4567 \
  ecr describe-repositories

# List images in a repository
aws --endpoint-url http://localhost:4567 \
  ecr list-images \
  --repository-name my-app

# Delete an image by tag
aws --endpoint-url http://localhost:4567 \
  ecr batch-delete-image \
  --repository-name my-app \
  --image-ids '[{"imageTag":"latest"}]'
```

## Notes

- The authorization token returned by `GetAuthorizationToken` is a valid base64-encoded string but does not perform real authentication.
- Image manifests are stored as-is; no image layer validation is performed.
- ECR does not integrate with a real Docker registry — `docker push/pull` commands require a real registry endpoint.
- State is in-memory only and lost on restart.
