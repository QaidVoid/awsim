/**
 * Pre-canned request templates for the playground. Each one fills
 * in method/path/headers/body so the user has a working starting
 * point and can edit from there.
 *
 * Authorization headers use a placeholder Credential — every awsim
 * service treats SIGv4 as advisory by default, so the literal value
 * doesn't have to be real for requests to flow through. The
 * `service` portion of the Credential string is what awsim's gateway
 * uses to route to the right ServiceHandler.
 */

export interface RequestTemplate {
  id: string;
  label: string;
  service: string;
  method: 'GET' | 'POST' | 'PUT' | 'DELETE' | 'HEAD' | 'PATCH';
  path: string;
  headers: { key: string; value: string }[];
  body: string;
}

const TODAY = '20260101';

function authHeader(service: string): { key: string; value: string } {
  return {
    key: 'Authorization',
    value: `AWS4-HMAC-SHA256 Credential=awsim-admin/${TODAY}/us-east-1/${service}/aws4_request, SignedHeaders=host;x-amz-date, Signature=placeholder`,
  };
}

function jsonTarget(target: string): { key: string; value: string }[] {
  return [
    { key: 'Content-Type', value: 'application/x-amz-json-1.0' },
    { key: 'X-Amz-Target', value: target },
  ];
}

export const TEMPLATES: RequestTemplate[] = [
  {
    id: 'sts-get-caller-identity',
    label: 'STS · GetCallerIdentity',
    service: 'sts',
    method: 'POST',
    path: '/',
    headers: [
      authHeader('sts'),
      { key: 'Content-Type', value: 'application/x-www-form-urlencoded' },
    ],
    body: 'Action=GetCallerIdentity&Version=2011-06-15',
  },
  {
    id: 's3-list-buckets',
    label: 'S3 · ListBuckets',
    service: 's3',
    method: 'GET',
    path: '/',
    headers: [authHeader('s3')],
    body: '',
  },
  {
    id: 's3-create-bucket',
    label: 'S3 · CreateBucket (my-bucket)',
    service: 's3',
    method: 'PUT',
    path: '/my-bucket',
    headers: [authHeader('s3')],
    body: '',
  },
  {
    id: 'dynamodb-list-tables',
    label: 'DynamoDB · ListTables',
    service: 'dynamodb',
    method: 'POST',
    path: '/',
    headers: [authHeader('dynamodb'), ...jsonTarget('DynamoDB_20120810.ListTables')],
    body: '{}',
  },
  {
    id: 'dynamodb-create-table',
    label: 'DynamoDB · CreateTable',
    service: 'dynamodb',
    method: 'POST',
    path: '/',
    headers: [authHeader('dynamodb'), ...jsonTarget('DynamoDB_20120810.CreateTable')],
    body: JSON.stringify(
      {
        TableName: 'my-table',
        AttributeDefinitions: [{ AttributeName: 'id', AttributeType: 'S' }],
        KeySchema: [{ AttributeName: 'id', KeyType: 'HASH' }],
        BillingMode: 'PAY_PER_REQUEST',
      },
      null,
      2,
    ),
  },
  {
    id: 'lambda-list-functions',
    label: 'Lambda · ListFunctions',
    service: 'lambda',
    method: 'GET',
    path: '/2015-03-31/functions/',
    headers: [authHeader('lambda')],
    body: '',
  },
  {
    id: 'sqs-list-queues',
    label: 'SQS · ListQueues',
    service: 'sqs',
    method: 'POST',
    path: '/',
    headers: [authHeader('sqs'), ...jsonTarget('AmazonSQS.ListQueues')],
    body: '{}',
  },
  {
    id: 'sns-list-topics',
    label: 'SNS · ListTopics',
    service: 'sns',
    method: 'POST',
    path: '/',
    headers: [
      authHeader('sns'),
      { key: 'Content-Type', value: 'application/x-www-form-urlencoded' },
    ],
    body: 'Action=ListTopics&Version=2010-03-31',
  },
  {
    id: 'kms-list-keys',
    label: 'KMS · ListKeys',
    service: 'kms',
    method: 'POST',
    path: '/',
    headers: [authHeader('kms'), ...jsonTarget('TrentService.ListKeys')],
    body: '{}',
  },
  {
    id: 'iam-list-users',
    label: 'IAM · ListUsers',
    service: 'iam',
    method: 'POST',
    path: '/',
    headers: [
      authHeader('iam'),
      { key: 'Content-Type', value: 'application/x-www-form-urlencoded' },
    ],
    body: 'Action=ListUsers&Version=2010-05-08',
  },
];
