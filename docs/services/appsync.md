# AppSync

AWS AppSync managed GraphQL service for building data-driven APIs with real-time and offline capabilities.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `RestJson1` |
| Signing Name | `appsync` |
| Persistence | No |

AppSync uses REST-style routing with JSON bodies. Paths follow the pattern `/v1/apis/{apiId}/...`.

## Quick Start

Create a GraphQL API, upload a schema, and create an API key for access:

```bash
# Create a GraphQL API
API_ID=$(curl -s -X POST http://localhost:4566/v1/apis \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/appsync/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"name":"my-graphql-api","authenticationType":"API_KEY"}' \
  | jq -r '.graphqlApi.apiId')

echo "API ID: $API_ID"

# Create an API key
curl -s -X POST http://localhost:4566/v1/apis/$API_ID/ApiKeys \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/appsync/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"description":"Dev key"}'
```

## Operations

### GraphQL APIs
- `CreateGraphqlApi` — create a new GraphQL API
  - Input: `name` (required), `authenticationType` (`API_KEY`, `AWS_IAM`, `OPENID_CONNECT`, `AMAZON_COGNITO_USER_POOLS`), `userPoolConfig`, `openIDConnectConfig`, `logConfig`, `tags`
  - Returns: `graphqlApi` object with `apiId`, `arn`, `uris` (GRAPHQL and REALTIME endpoints)

- `GetGraphqlApi` — get a specific GraphQL API by ID
  - Input: `apiId`
  - Returns: full `graphqlApi` object

- `ListGraphqlApis` — list all GraphQL APIs
  - Input: optional `maxResults`, `nextToken`
  - Returns: paginated `graphqlApis` list

- `DeleteGraphqlApi` — delete a GraphQL API and all associated resources
  - Input: `apiId`

- `UpdateGraphqlApi` — update API name or authentication configuration
  - Input: `apiId`, plus any fields to update

### Schemas
- `StartSchemaCreation` — upload a GraphQL schema definition (SDL format, base64-encoded)
  - Input: `apiId`, `definition` (base64-encoded SDL string)
  - Returns: `status` (`PROCESSING`, then `ACTIVE`)

- `GetSchemaCreationStatus` — poll the status of an in-progress schema upload
  - Input: `apiId`
  - Returns: `status` and `details`

### API Keys
- `CreateApiKey` — create an API key for `API_KEY` authentication
  - Input: `apiId`, optional `description`, `expires` (Unix timestamp)
  - Returns: `apiKey` with `id` and `value` (the secret key string)

- `ListApiKeys` — list all API keys for a GraphQL API
  - Input: `apiId`
  - Returns: `apiKeys` list

- `DeleteApiKey` — delete an API key
  - Input: `apiId`, `id`

### Data Sources
- `CreateDataSource` — attach a backend data source (Lambda, DynamoDB, HTTP, etc.)
  - Input: `apiId`, `name`, `type` (`AWS_LAMBDA`, `AMAZON_DYNAMODB`, `HTTP`, `NONE`), `lambdaConfig` or `dynamodbConfig` or `httpConfig`
  - Returns: `dataSource` with `dataSourceArn`

- `ListDataSources` — list data sources for a GraphQL API
- `DeleteDataSource` — remove a data source

### Resolvers
- `CreateResolver` — map a GraphQL type/field to a data source
  - Input: `apiId`, `typeName` (e.g., `Query`), `fieldName` (e.g., `getUser`), `dataSourceName`, `requestMappingTemplate`, `responseMappingTemplate`
  - Returns: `resolver` object

- `ListResolvers` — list resolvers for a type in a GraphQL API

## Curl Examples

```bash
# 1. Create a GraphQL API with Cognito authentication
curl -s -X POST http://localhost:4566/v1/apis \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/appsync/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"name":"my-api","authenticationType":"AMAZON_COGNITO_USER_POOLS","userPoolConfig":{"userPoolId":"us-east-1_abc123","awsRegion":"us-east-1","defaultAction":"ALLOW"}}'

# 2. Upload a schema (schema SDL must be base64-encoded)
SCHEMA='type Query { users: [String] }'
ENCODED=$(echo -n "$SCHEMA" | base64)
curl -s -X POST http://localhost:4566/v1/apis/YOUR_API_ID/schemacreation \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/appsync/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"definition\":\"$ENCODED\"}"

# 3. Create a Lambda data source
curl -s -X POST http://localhost:4566/v1/apis/YOUR_API_ID/datasources \
  -H "Content-Type: application/json" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/appsync/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"name":"userLambda","type":"AWS_LAMBDA","serviceRoleArn":"arn:aws:iam::000000000000:role/AppSyncRole","lambdaConfig":{"lambdaFunctionArn":"arn:aws:lambda:us-east-1:000000000000:function:get-users"}}'
```

## SDK Example

```typescript
import {
  AppSyncClient,
  CreateGraphqlApiCommand,
  StartSchemaCreationCommand,
  CreateApiKeyCommand,
  CreateDataSourceCommand,
} from '@aws-sdk/client-appsync';

const appsync = new AppSyncClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create GraphQL API
const { graphqlApi } = await appsync.send(new CreateGraphqlApiCommand({
  name: 'my-api',
  authenticationType: 'API_KEY',
}));

const apiId = graphqlApi!.apiId!;

// Upload schema
const schema = 'type Query { users: [String] }';
await appsync.send(new StartSchemaCreationCommand({
  apiId,
  definition: Buffer.from(schema),
}));

// Create API key
const { apiKey } = await appsync.send(new CreateApiKeyCommand({ apiId }));
console.log('API Key:', apiKey?.id, apiKey?.value);

// Create Lambda data source
await appsync.send(new CreateDataSourceCommand({
  apiId,
  name: 'myLambda',
  type: 'AWS_LAMBDA',
  serviceRoleArn: 'arn:aws:iam::000000000000:role/AppSyncRole',
  lambdaConfig: {
    lambdaFunctionArn: 'arn:aws:lambda:us-east-1:000000000000:function:my-fn',
  },
}));
```

## Behavior Notes

- AppSync in AWSim manages API metadata, schemas, data sources, and resolvers but does **not** execute GraphQL queries.
- Schema definitions are stored as-is (base64-encoded) and returned as-is — no SDL parsing or validation is performed.
- Real GraphQL request execution is not supported; use AWSim AppSync for SDK configuration testing and IaC validation.
- State is in-memory only and lost on restart.
- The `uris` field in `CreateGraphqlApi` response includes a placeholder GraphQL endpoint URL.
