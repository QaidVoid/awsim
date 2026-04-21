/// AWS API protocols.
///
/// Each AWS service uses one of these protocols for request/response serialization.
/// The protocol determines how operations are identified, how requests are parsed,
/// and how responses are formatted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// JSON-RPC style (DynamoDB, KMS, STS via JSON).
    /// Operation identified by `X-Amz-Target` header.
    /// Content-Type: `application/x-amz-json-1.0`
    AwsJson1_0,

    /// JSON-RPC style (ECS, CloudWatch Logs, Secrets Manager, Cognito).
    /// Operation identified by `X-Amz-Target` header.
    /// Content-Type: `application/x-amz-json-1.1`
    AwsJson1_1,

    /// RESTful JSON (Lambda, API Gateway, EventBridge).
    /// Operation identified by HTTP method + URI path.
    /// Content-Type: `application/json`
    RestJson1,

    /// RESTful XML (S3, CloudFront, Route 53).
    /// Operation identified by HTTP method + URI path.
    /// Content-Type: `application/xml`
    RestXml,

    /// Legacy query protocol (IAM, STS, SQS legacy, CloudFormation).
    /// Operation identified by `Action` form parameter.
    /// Content-Type: `application/x-www-form-urlencoded`
    /// Response: XML
    AwsQuery,

    /// EC2-specific query protocol variant.
    /// Similar to AwsQuery but with EC2-specific serialization quirks.
    Ec2Query,
}

impl Protocol {
    /// Returns the Content-Type header value for request detection.
    pub fn content_type(&self) -> &'static str {
        match self {
            Self::AwsJson1_0 => "application/x-amz-json-1.0",
            Self::AwsJson1_1 => "application/x-amz-json-1.1",
            Self::RestJson1 => "application/json",
            Self::RestXml => "application/xml",
            Self::AwsQuery | Self::Ec2Query => "application/x-www-form-urlencoded",
        }
    }

    /// Returns the Content-Type for responses.
    pub fn response_content_type(&self) -> &'static str {
        match self {
            Self::AwsJson1_0 | Self::AwsJson1_1 | Self::RestJson1 => "application/json",
            Self::RestXml | Self::AwsQuery | Self::Ec2Query => "application/xml",
        }
    }
}
