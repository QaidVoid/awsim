import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'AWSim',
  description: 'Fully offline, free AWS development environment',
  themeConfig: {
    nav: [
      { text: 'Guide', link: '/guide/getting-started' },
      { text: 'Services', link: '/services/' },
      { text: 'GitHub', link: 'https://github.com/qaidvoid/awsim' }
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
            { text: 'Cross-Service Integrations', link: '/guide/integrations' },
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
          text: 'Services',
          items: [
            { text: 'Overview', link: '/services/' },
            { text: 'S3', link: '/services/s3' },
            { text: 'DynamoDB', link: '/services/dynamodb' },
            { text: 'SQS', link: '/services/sqs' },
            { text: 'SNS', link: '/services/sns' },
            { text: 'Lambda', link: '/services/lambda' },
            { text: 'Cognito', link: '/services/cognito' },
            { text: 'IAM & STS', link: '/services/iam' },
          ]
        }
      ]
    },
    socialLinks: [
      { icon: 'github', link: 'https://github.com/qaidvoid/awsim' }
    ],
    footer: {
      message: 'Released under MIT / Apache-2.0 License',
    }
  }
})
