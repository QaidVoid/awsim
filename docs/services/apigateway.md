# API Gateway

Amazon API Gateway v2 (HTTP APIs) for creating, deploying, and managing REST and HTTP APIs backed by Lambda or HTTP integrations.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `RestJson1` |
| Signing Name | `execute-api` |
| Persistence | No |

## Operations

### APIs
- `CreateApi` — create a new HTTP API
- `GetApi` — get a specific API by ID
- `GetApis` — list all APIs
- `DeleteApi` — delete an API
- `UpdateApi` — update API name, description, or CORS configuration

### Routes
- `CreateRoute` — create a route (e.g., `GET /users/{id}`) on an API
- `GetRoute` — get a specific route
- `GetRoutes` — list all routes for an API
- `DeleteRoute` — delete a route

### Integrations
- `CreateIntegration` — create an integration (Lambda, HTTP proxy) for a route
- `GetIntegration` — get a specific integration
- `DeleteIntegration` — delete an integration

### Stages
- `CreateStage` — create a deployment stage (e.g., `$default`, `prod`)
- `GetStage` — get a specific stage
- `GetStages` — list all stages for an API
- `DeleteStage` — delete a stage

### Deployments
- `CreateDeployment` — deploy an API to a stage
- `GetDeployment` — get a specific deployment

## Example

```bash
# Create an HTTP API
aws --endpoint-url http://localhost:4567 \
  apigatewayv2 create-api \
  --name my-api \
  --protocol-type HTTP

# Create a Lambda integration
aws --endpoint-url http://localhost:4567 \
  apigatewayv2 create-integration \
  --api-id <api-id> \
  --integration-type AWS_PROXY \
  --integration-uri arn:aws:lambda:us-east-1:000000000000:function:my-fn \
  --payload-format-version 2.0

# Create a route
aws --endpoint-url http://localhost:4567 \
  apigatewayv2 create-route \
  --api-id <api-id> \
  --route-key "GET /hello" \
  --target integrations/<integration-id>

# Create default stage
aws --endpoint-url http://localhost:4567 \
  apigatewayv2 create-stage \
  --api-id <api-id> \
  --stage-name '$default' \
  --auto-deploy true
```

## Notes

- AWSim includes an API Gateway proxy that routes HTTP requests to registered APIs and forwards them to the configured integration target (Lambda functions).
- Configured APIs are accessible at `http://localhost:4567/execute-api/{api-id}/{stage}/{path}`.
- Lambda integrations actually invoke the Lambda function via AWSim's Lambda service.
- HTTP integrations forward requests to the configured upstream URL.
