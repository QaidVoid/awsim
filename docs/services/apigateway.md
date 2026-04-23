# API Gateway

Amazon API Gateway v2 (HTTP APIs) for creating, deploying, and managing REST and HTTP APIs backed by Lambda or HTTP integrations.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `RestJson1` |
| Signing Name | `execute-api` |
| Persistence | No |

API Gateway v2 uses REST-style routing with JSON bodies. Paths follow the pattern `/v2/apis/{apiId}/...`.

## Quick Start

Create an API, add a Lambda integration, create a route, and deploy it:

```bash
# Create an HTTP API
API_ID=$(curl -s -X POST http://localhost:4566/v2/apis \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/execute-api/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"my-api","ProtocolType":"HTTP"}' \
  | jq -r '.ApiId')

echo "API ID: $API_ID"

# Create a Lambda integration
INTEGRATION_ID=$(curl -s -X POST http://localhost:4566/v2/apis/$API_ID/integrations \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/execute-api/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"IntegrationType":"AWS_PROXY","IntegrationUri":"arn:aws:lambda:us-east-1:000000000000:function:my-fn","PayloadFormatVersion":"2.0"}' \
  | jq -r '.IntegrationId')

# Create a route pointing to the integration
curl -s -X POST http://localhost:4566/v2/apis/$API_ID/routes \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/execute-api/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"RouteKey\":\"GET /hello\",\"Target\":\"integrations/$INTEGRATION_ID\"}"
```

## Operations

### APIs
- `CreateApi` — create a new HTTP or WebSocket API
  - Input: `Name` (required), `ProtocolType` (`HTTP` or `WEBSOCKET`), `Description`, `CorsConfiguration`, `RouteSelectionExpression`
  - Returns: `ApiId`, `ApiEndpoint`, `CreatedDate`

- `GetApi` — get a specific API by ID
  - Input: `ApiId`
  - Returns full API metadata including `ApiEndpoint` and CORS config

- `GetApis` — list all APIs
  - Input: optional `MaxResults`, `NextToken`
  - Returns: paginated `Items` list

- `DeleteApi` — delete an API and all associated routes, integrations, and stages
  - Input: `ApiId`

- `UpdateApi` — update API name, description, or CORS configuration
  - Input: `ApiId` plus any fields to update

### Routes
- `CreateRoute` — create a route with a method and path
  - Input: `ApiId`, `RouteKey` (e.g., `GET /users/{id}`, `$default`), `Target` (e.g., `integrations/{integrationId}`)
  - Returns: `RouteId`, `RouteKey`, `Target`

- `GetRoute` / `GetRoutes` — retrieve one or all routes for an API
- `DeleteRoute` — delete a route by ID

### Integrations
- `CreateIntegration` — create a backend integration for a route
  - Input: `ApiId`, `IntegrationType` (`AWS_PROXY`, `HTTP_PROXY`), `IntegrationUri`, `PayloadFormatVersion` (`1.0` or `2.0`)
  - Returns: `IntegrationId`

- `GetIntegration` / `DeleteIntegration` — retrieve or remove an integration

### Stages
- `CreateStage` — create a deployment stage (e.g., `$default`, `prod`)
  - Input: `ApiId`, `StageName`, `AutoDeploy` (boolean)
  - Returns: `StageName`, `CreatedDate`

- `GetStage` / `GetStages` / `DeleteStage` — retrieve or remove stages

### Deployments
- `CreateDeployment` — deploy the current API configuration to a stage
  - Input: `ApiId`, optional `StageName`
  - Returns: `DeploymentId`, `CreatedDate`

- `GetDeployment` — get deployment status and metadata

## Curl Examples

```bash
# 1. Create an HTTP API
curl -s -X POST http://localhost:4566/v2/apis \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/execute-api/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Name":"users-api","ProtocolType":"HTTP","Description":"User management API"}'

# 2. List all APIs
curl -s http://localhost:4566/v2/apis \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/execute-api/aws4_request, SignedHeaders=host, Signature=fake"

# 3. Create a default stage with auto-deploy
curl -s -X POST http://localhost:4566/v2/apis/YOUR_API_ID/stages \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/execute-api/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"StageName":"$default","AutoDeploy":true}'
```

## SDK Example

```typescript
import {
  ApiGatewayV2Client,
  CreateApiCommand,
  CreateIntegrationCommand,
  CreateRouteCommand,
  CreateStageCommand,
} from '@aws-sdk/client-apigatewayv2';

const apigw = new ApiGatewayV2Client({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create HTTP API
const { ApiId } = await apigw.send(new CreateApiCommand({
  Name: 'my-api',
  ProtocolType: 'HTTP',
}));

// Create Lambda integration
const { IntegrationId } = await apigw.send(new CreateIntegrationCommand({
  ApiId,
  IntegrationType: 'AWS_PROXY',
  IntegrationUri: 'arn:aws:lambda:us-east-1:000000000000:function:my-fn',
  PayloadFormatVersion: '2.0',
}));

// Create a route
await apigw.send(new CreateRouteCommand({
  ApiId,
  RouteKey: 'GET /hello',
  Target: `integrations/${IntegrationId}`,
}));

// Create default stage
await apigw.send(new CreateStageCommand({
  ApiId,
  StageName: '$default',
  AutoDeploy: true,
}));

// The API is now accessible at: http://localhost:4566/execute-api/{ApiId}/$default/hello
console.log(`Invoke at: http://localhost:4566/execute-api/${ApiId}/$default/hello`);
```

## Behavior Notes

- AWSim includes an API Gateway proxy that routes incoming requests to registered APIs and forwards them to the configured Lambda integration.
- Deployed APIs are accessible at `http://localhost:4566/execute-api/{apiId}/{stage}/{path}`.
- Lambda integrations actually invoke the Lambda function through AWSim's Lambda service — responses come back in real-time.
- HTTP proxy integrations forward requests to the configured upstream URL; ensure the upstream is reachable from the AWSim host.
- State is in-memory only and lost on restart.
