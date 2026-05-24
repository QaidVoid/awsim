import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'AWSim',
  description: 'Fully offline, free AWS development environment',
  themeConfig: {
    nav: [
      { text: 'Guide', link: '/guide/getting-started' },
      { text: 'Services', link: '/services/' },
      { text: 'GitHub', link: 'https://github.com/QaidVoid/awsim' }
    ],
    sidebar: {
      '/guide/': [
        {
          text: 'Introduction',
          items: [
            { text: 'What is AWSim?', link: '/guide/what-is-awsim' },
            { text: 'Getting Started', link: '/guide/getting-started' },
            { text: 'Configuration', link: '/guide/configuration' },
          ]
        },
        {
          text: 'Features',
          items: [
            { text: 'Persistence', link: '/guide/persistence' },
            { text: 'Admin Console', link: '/guide/admin-console' },
            { text: 'Operator Auth', link: '/guide/operator-auth' },
            { text: 'Estimated Billing', link: '/guide/billing' },
            { text: 'Chaos Engine', link: '/guide/chaos' },
            { text: 'Cross-Service Integrations', link: '/guide/integrations' },
            { text: 'IAM Policy Enforcement', link: '/guide/iam-enforcement' },
            { text: 'Cognito OAuth/OIDC', link: '/guide/cognito-oauth' },
            { text: 'Cognito Federation (OIDC IdP)', link: '/guide/cognito-federation' },
            { text: 'Lambda Execution', link: '/guide/lambda-execution' },
            { text: 'API Gateway Proxy', link: '/guide/api-gateway' },
            { text: 'OpenSearch', link: '/guide/opensearch' },
            { text: 'Bedrock LLM Backend', link: '/guide/bedrock-backend' },
            { text: 'Seeding Test Data', link: '/guide/seeding' },
            { text: 'Memory & Diagnostics', link: '/guide/memory' },
          ]
        },
        {
          text: 'Deployment',
          items: [
            { text: 'Docker', link: '/guide/docker' },
            { text: 'HTTPS / TLS', link: '/guide/tls' },
            { text: 'Nix', link: '/guide/nix' },
          ]
        }
      ],
      '/services/': [
        {
          text: 'Core Services',
          items: [
            { text: 'Overview', link: '/services/' },
            { text: 'S3', link: '/services/s3' },
            { text: 'DynamoDB', link: '/services/dynamodb' },
            { text: 'SQS', link: '/services/sqs' },
            { text: 'SNS', link: '/services/sns' },
            { text: 'Lambda', link: '/services/lambda' },
            { text: 'IAM & STS', link: '/services/iam' },
          ]
        },
        {
          text: 'Compute, Events & Observability',
          items: [
            { text: 'API Gateway', link: '/services/apigateway' },
            { text: 'EventBridge', link: '/services/eventbridge' },
            { text: 'Pipes', link: '/services/pipes' },
            { text: 'Scheduler', link: '/services/scheduler' },
            { text: 'Step Functions', link: '/services/stepfunctions' },
            { text: 'MQ', link: '/services/mq' },
            { text: 'CloudWatch Logs', link: '/services/cloudwatch-logs' },
            { text: 'CloudWatch Metrics', link: '/services/cloudwatch-metrics' },
            { text: 'X-Ray', link: '/services/xray' },
          ]
        },
        {
          text: 'Security, Identity & Config',
          items: [
            { text: 'Cognito', link: '/services/cognito' },
            { text: 'Identity Store', link: '/services/identitystore' },
            { text: 'KMS', link: '/services/kms' },
            { text: 'Secrets Manager', link: '/services/secretsmanager' },
            { text: 'SSM Parameter Store', link: '/services/ssm' },
            { text: 'AppConfig', link: '/services/appconfig' },
            { text: 'ACM', link: '/services/acm' },
            { text: 'WAF', link: '/services/waf' },
          ]
        },
        {
          text: 'Data & Analytics',
          items: [
            { text: 'Kinesis', link: '/services/kinesis' },
            { text: 'SES', link: '/services/ses' },
            { text: 'Pinpoint', link: '/services/pinpoint' },
            { text: 'OpenSearch', link: '/services/opensearch' },
            { text: 'Kendra', link: '/services/kendra' },
            { text: 'Comprehend', link: '/services/comprehend' },
            { text: 'Athena', link: '/services/athena' },
            { text: 'Glue', link: '/services/glue' },
            { text: 'Bedrock', link: '/services/bedrock' },
          ]
        },
        {
          text: 'Databases',
          items: [
            { text: 'RDS', link: '/services/rds' },
            { text: 'DocumentDB', link: '/services/docdb' },
            { text: 'Neptune', link: '/services/neptune' },
            { text: 'MemoryDB', link: '/services/memorydb' },
            { text: 'QLDB', link: '/services/qldb' },
          ]
        },
        {
          text: 'Storage & Transfer',
          items: [
            { text: 'EFS', link: '/services/efs' },
            { text: 'Glacier', link: '/services/glacier' },
            { text: 'Backup', link: '/services/backup' },
            { text: 'Transfer Family', link: '/services/transfer' },
          ]
        },
        {
          text: 'Compute & Networking',
          items: [
            { text: 'EC2', link: '/services/ec2' },
            { text: 'ECS', link: '/services/ecs' },
            { text: 'ECR', link: '/services/ecr' },
            { text: 'ELB', link: '/services/elb' },
            { text: 'CloudFront', link: '/services/cloudfront' },
            { text: 'Route 53', link: '/services/route53' },
            { text: 'CloudFormation', link: '/services/cloudformation' },
            { text: 'AppSync', link: '/services/appsync' },
            { text: 'Service Discovery', link: '/services/servicediscovery' },
            { text: 'Application Auto Scaling', link: '/services/application-autoscaling' },
            { text: 'Resource Groups Tagging', link: '/services/resourcegroupstagging' },
          ]
        }
      ]
    },
    socialLinks: [
      { icon: 'github', link: 'https://github.com/QaidVoid/awsim' }
    ],
    footer: {
      message: 'Released under MIT / Apache-2.0 License',
    }
  }
})
