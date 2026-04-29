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
            { text: 'Estimated Billing', link: '/guide/billing' },
            { text: 'Chaos Engine', link: '/guide/chaos' },
            { text: 'Cross-Service Integrations', link: '/guide/integrations' },
            { text: 'IAM Policy Enforcement', link: '/guide/iam-enforcement' },
            { text: 'Cognito OAuth/OIDC', link: '/guide/cognito-oauth' },
            { text: 'Lambda Execution', link: '/guide/lambda-execution' },
            { text: 'API Gateway Proxy', link: '/guide/api-gateway' },
            { text: 'OpenSearch', link: '/guide/opensearch' },
          ]
        },
        {
          text: 'Deployment',
          items: [
            { text: 'Docker', link: '/guide/docker' },
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
          text: 'Compute & Events',
          items: [
            { text: 'API Gateway', link: '/services/apigateway' },
            { text: 'EventBridge', link: '/services/eventbridge' },
            { text: 'Step Functions', link: '/services/stepfunctions' },
            { text: 'CloudWatch Logs', link: '/services/cloudwatch-logs' },
            { text: 'CloudWatch Metrics', link: '/services/cloudwatch-metrics' },
          ]
        },
        {
          text: 'Security & Config',
          items: [
            { text: 'Cognito', link: '/services/cognito' },
            { text: 'KMS', link: '/services/kms' },
            { text: 'Secrets Manager', link: '/services/secretsmanager' },
            { text: 'SSM Parameter Store', link: '/services/ssm' },
            { text: 'ACM', link: '/services/acm' },
            { text: 'WAF', link: '/services/waf' },
          ]
        },
        {
          text: 'Data & Analytics',
          items: [
            { text: 'Kinesis', link: '/services/kinesis' },
            { text: 'SES', link: '/services/ses' },
            { text: 'OpenSearch', link: '/services/opensearch' },
            { text: 'Kendra', link: '/services/kendra' },
            { text: 'Comprehend', link: '/services/comprehend' },
            { text: 'Athena', link: '/services/athena' },
            { text: 'Glue', link: '/services/glue' },
            { text: 'Bedrock', link: '/services/bedrock' },
          ]
        },
        {
          text: 'Infrastructure',
          items: [
            { text: 'EC2', link: '/services/ec2' },
            { text: 'ECS', link: '/services/ecs' },
            { text: 'ECR', link: '/services/ecr' },
            { text: 'ELB', link: '/services/elb' },
            { text: 'RDS', link: '/services/rds' },
            { text: 'CloudFront', link: '/services/cloudfront' },
            { text: 'Route 53', link: '/services/route53' },
            { text: 'CloudFormation', link: '/services/cloudformation' },
            { text: 'AppSync', link: '/services/appsync' },
            { text: 'Scheduler', link: '/services/scheduler' },
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
