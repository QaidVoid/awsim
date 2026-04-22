# API Gateway

AWSim emulates API Gateway (REST APIs) including the management API and a live proxy endpoint.

## Management API

Use the standard AWS API Gateway management operations:

```bash
# Create a REST API
aws --endpoint-url http://localhost:4566 apigateway create-rest-api \
  --name my-api

# List resources
aws --endpoint-url http://localhost:4566 apigateway get-resources \
  --rest-api-id <api_id>

# Create a resource
aws --endpoint-url http://localhost:4566 apigateway create-resource \
  --rest-api-id <api_id> \
  --parent-id <root_resource_id> \
  --path-part users

# Create a method
aws --endpoint-url http://localhost:4566 apigateway put-method \
  --rest-api-id <api_id> \
  --resource-id <resource_id> \
  --http-method GET \
  --authorization-type NONE

# Create a Lambda integration
aws --endpoint-url http://localhost:4566 apigateway put-integration \
  --rest-api-id <api_id> \
  --resource-id <resource_id> \
  --http-method GET \
  --type AWS_PROXY \
  --integration-http-method POST \
  --uri arn:aws:apigateway:us-east-1:lambda:path/2015-03-31/functions/arn:aws:lambda:us-east-1:000000000000:function:my-function/invocations

# Deploy
aws --endpoint-url http://localhost:4566 apigateway create-deployment \
  --rest-api-id <api_id> \
  --stage-name dev
```

## Proxy Routing

Once deployed, your API is accessible at:

```
/restapis/{api_id}/{stage}/{path}
```

For example, a `GET /users` route deployed to stage `dev` with API ID `abc123` is reachable at:

```
GET http://localhost:4566/restapis/abc123/dev/users
```

## Lambda Proxy Integration

AWSim uses the Lambda Payload Format v2 for proxy integrations. Your Lambda handler receives:

```json
{
  "version": "2.0",
  "routeKey": "GET /users",
  "rawPath": "/users",
  "rawQueryString": "page=1",
  "headers": { "content-type": "application/json" },
  "requestContext": {
    "http": {
      "method": "GET",
      "path": "/users",
      "sourceIp": "127.0.0.1"
    },
    "stage": "dev"
  },
  "body": null,
  "isBase64Encoded": false
}
```

Your handler should return:

```json
{
  "statusCode": 200,
  "headers": { "content-type": "application/json" },
  "body": "{\"users\":[]}"
}
```

## Notes

- Only `AWS_PROXY` (Lambda proxy) integrations are fully supported.
- HTTP integrations (forwarding to external URLs) are not implemented.
- API Gateway authorizers (JWT, Lambda) are accepted but not enforced.
