# AppSync

AWS AppSync managed GraphQL service for building data-driven APIs with real-time and offline capabilities.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `RestJson1` |
| Signing Name | `appsync` |
| Persistence | No |

## Operations

### GraphQL APIs
- `CreateGraphqlApi` — create a new GraphQL API
- `GetGraphqlApi` — get a specific GraphQL API by ID
- `ListGraphqlApis` — list all GraphQL APIs
- `DeleteGraphqlApi` — delete a GraphQL API
- `UpdateGraphqlApi` — update API name or authentication configuration

### Schemas
- `StartSchemaCreation` — upload a GraphQL schema definition
- `GetSchemaCreationStatus` — check the status of an in-progress schema creation

### API Keys
- `CreateApiKey` — create an API key for the API_KEY authentication type
- `ListApiKeys` — list API keys for a GraphQL API
- `DeleteApiKey` — delete an API key

### Data Sources
- `CreateDataSource` — attach a data source (Lambda, DynamoDB, HTTP, etc.)
- `ListDataSources` — list data sources for a GraphQL API
- `DeleteDataSource` — remove a data source

### Resolvers
- `CreateResolver` — create a field resolver mapping a type/field to a data source
- `ListResolvers` — list resolvers for a type in a GraphQL API

## Example

```bash
# Create a GraphQL API
aws --endpoint-url http://localhost:4567 \
  appsync create-graphql-api \
  --name my-api \
  --authentication-type API_KEY

# Upload a schema
aws --endpoint-url http://localhost:4567 \
  appsync start-schema-creation \
  --api-id <api-id> \
  --definition "dHlwZSBRdWVyeSB7IHVzZXJzOiBbU3RyaW5nXSB9"

# Create an API key
aws --endpoint-url http://localhost:4567 \
  appsync create-api-key \
  --api-id <api-id>

# Create a Lambda data source
aws --endpoint-url http://localhost:4567 \
  appsync create-data-source \
  --api-id <api-id> \
  --name myLambda \
  --type AWS_LAMBDA \
  --lambda-config '{"lambdaFunctionArn":"arn:aws:lambda:us-east-1:000000000000:function:my-fn"}'
```

## Notes

- AppSync in AWSim manages API metadata, schemas, data sources, and resolvers but does not execute GraphQL queries.
- Schema definitions are stored as-is (base64-encoded) and returned as-is — no SDL parsing or validation is performed.
- Real GraphQL request execution is not supported; use AWSim AppSync for configuration and testing SDK setup only.
- State is in-memory only and lost on restart.
