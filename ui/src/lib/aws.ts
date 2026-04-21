const ENDPOINT = 'http://localhost:4566';

const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, '');

function authHeader(service: string): string {
    return `AWS4-HMAC-SHA256 Credential=test/${FAKE_DATE}/us-east-1/${service}/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

async function awsRequest(
    service: string,
    action: string,
    params: Record<string, string> = {},
    protocol: 'query' | 'json' = 'json',
    targetPrefix?: string
): Promise<unknown> {
    if (protocol === 'query') {
        const body = new URLSearchParams({ Action: action, Version: '2010-05-08', ...params });
        const res = await fetch(ENDPOINT, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/x-www-form-urlencoded',
                'Authorization': authHeader(service),
                'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
            },
            body: body.toString(),
        });
        const text = await res.text();
        return { ok: res.ok, text, status: res.status };
    } else {
        const target = targetPrefix ? `${targetPrefix}.${action}` : action;
        const res = await fetch(ENDPOINT, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/x-amz-json-1.0',
                'X-Amz-Target': target,
                'Authorization': authHeader(service),
                'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
            },
            body: JSON.stringify(params),
        });
        if (!res.ok) {
            throw new Error(`HTTP ${res.status}: ${await res.text()}`);
        }
        return res.json();
    }
}

// ---- S3 ----

export interface S3Bucket {
    name: string;
    creationDate: string;
}

function parseXmlListBuckets(xml: string): { buckets: S3Bucket[] } {
    const buckets: S3Bucket[] = [];
    const regex = /<Name>([^<]+)<\/Name>\s*<CreationDate>([^<]+)<\/CreationDate>/g;
    let match;
    while ((match = regex.exec(xml)) !== null) {
        buckets.push({ name: match[1], creationDate: match[2] });
    }
    return { buckets };
}

export async function listBuckets(): Promise<{ buckets: S3Bucket[] }> {
    const res = await fetch(`${ENDPOINT}/`, {
        headers: {
            'Authorization': authHeader('s3'),
            'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
        },
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const text = await res.text();
    return parseXmlListBuckets(text);
}

function s3Headers(): Record<string, string> {
    return {
        'Authorization': authHeader('s3'),
        'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
    };
}

export async function createBucket(name: string): Promise<void> {
    const res = await fetch(`${ENDPOINT}/${encodeURIComponent(name)}`, {
        method: 'PUT',
        headers: s3Headers(),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}

export async function deleteBucket(name: string): Promise<void> {
    const res = await fetch(`${ENDPOINT}/${encodeURIComponent(name)}`, {
        method: 'DELETE',
        headers: s3Headers(),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}

export interface S3Object {
    key: string;
    size: number;
    lastModified: string;
    etag: string;
}

export interface S3CommonPrefix {
    prefix: string;
}

export interface ListObjectsResult {
    objects: S3Object[];
    commonPrefixes: S3CommonPrefix[];
    isTruncated: boolean;
    nextContinuationToken?: string;
}

function parseXmlListObjects(xml: string): ListObjectsResult {
    const objects: S3Object[] = [];
    const commonPrefixes: S3CommonPrefix[] = [];

    const contentRegex = /<Contents>([\s\S]*?)<\/Contents>/g;
    let match;
    while ((match = contentRegex.exec(xml)) !== null) {
        const block = match[1];
        const key = (/<Key>([^<]+)<\/Key>/.exec(block) ?? [])[1] ?? '';
        const size = parseInt((/<Size>([^<]+)<\/Size>/.exec(block) ?? [])[1] ?? '0', 10);
        const lastModified = (/<LastModified>([^<]+)<\/LastModified>/.exec(block) ?? [])[1] ?? '';
        const etag = (/<ETag>([^<]+)<\/ETag>/.exec(block) ?? [])[1] ?? '';
        objects.push({ key, size, lastModified, etag: etag.replace(/&quot;/g, '"') });
    }

    const prefixRegex = /<CommonPrefixes>\s*<Prefix>([^<]+)<\/Prefix>\s*<\/CommonPrefixes>/g;
    while ((match = prefixRegex.exec(xml)) !== null) {
        commonPrefixes.push({ prefix: match[1] });
    }

    const isTruncated = /<IsTruncated>true<\/IsTruncated>/i.test(xml);
    const tokenMatch = /<NextContinuationToken>([^<]+)<\/NextContinuationToken>/.exec(xml);

    return {
        objects,
        commonPrefixes,
        isTruncated,
        nextContinuationToken: tokenMatch ? tokenMatch[1] : undefined,
    };
}

export async function listObjects(bucket: string, prefix?: string, delimiter?: string): Promise<ListObjectsResult> {
    const params = new URLSearchParams({ 'list-type': '2' });
    if (prefix) params.set('prefix', prefix);
    if (delimiter !== undefined) params.set('delimiter', delimiter);

    const res = await fetch(`${ENDPOINT}/${encodeURIComponent(bucket)}?${params.toString()}`, {
        headers: s3Headers(),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
    const text = await res.text();
    return parseXmlListObjects(text);
}

export async function deleteObject(bucket: string, key: string): Promise<void> {
    const res = await fetch(`${ENDPOINT}/${encodeURIComponent(bucket)}/${key.split('/').map(encodeURIComponent).join('/')}`, {
        method: 'DELETE',
        headers: s3Headers(),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}

// ---- SQS ----

export interface SqsQueue {
    url: string;
    name: string;
}

export interface SqsQueueAttributes {
    approximateNumberOfMessages: number;
    approximateNumberOfMessagesNotVisible: number;
    createdTimestamp: string;
    visibilityTimeout: number;
    messageRetentionPeriod: number;
    isFifo: boolean;
}

export interface SqsMessage {
    messageId: string;
    receiptHandle: string;
    body: string;
    attributes: Record<string, string>;
}

export async function listQueues(): Promise<{ queues: SqsQueue[] }> {
    const data = await awsRequest('sqs', 'ListQueues', {}, 'json', 'AmazonSQS') as { QueueUrls?: string[] };
    const urls = data.QueueUrls ?? [];
    return {
        queues: urls.map((url) => ({
            url,
            name: url.split('/').pop() ?? url,
        })),
    };
}

export async function getQueueAttributes(queueUrl: string): Promise<SqsQueueAttributes> {
    const data = await awsRequest('sqs', 'GetQueueAttributes', {
        QueueUrl: queueUrl,
        AttributeNames: ['All'],
    } as unknown as Record<string, string>, 'json', 'AmazonSQS') as {
        Attributes?: Record<string, string>
    };
    const attrs = data.Attributes ?? {};
    return {
        approximateNumberOfMessages: parseInt(attrs['ApproximateNumberOfMessages'] ?? '0', 10),
        approximateNumberOfMessagesNotVisible: parseInt(attrs['ApproximateNumberOfMessagesNotVisible'] ?? '0', 10),
        createdTimestamp: attrs['CreatedTimestamp'] ?? '',
        visibilityTimeout: parseInt(attrs['VisibilityTimeout'] ?? '30', 10),
        messageRetentionPeriod: parseInt(attrs['MessageRetentionPeriod'] ?? '345600', 10),
        isFifo: attrs['FifoQueue'] === 'true',
    };
}

export async function createQueue(name: string, fifo = false): Promise<{ queueUrl: string }> {
    const params: Record<string, unknown> = { QueueName: fifo ? (name.endsWith('.fifo') ? name : `${name}.fifo`) : name };
    if (fifo) {
        params['Attributes'] = { FifoQueue: 'true' };
    }
    const data = await awsRequest('sqs', 'CreateQueue', params as unknown as Record<string, string>, 'json', 'AmazonSQS') as { QueueUrl?: string };
    return { queueUrl: data.QueueUrl ?? '' };
}

export async function deleteQueue(queueUrl: string): Promise<void> {
    await awsRequest('sqs', 'DeleteQueue', { QueueUrl: queueUrl } as unknown as Record<string, string>, 'json', 'AmazonSQS');
}

export async function sendMessage(queueUrl: string, body: string): Promise<void> {
    await awsRequest('sqs', 'SendMessage', { QueueUrl: queueUrl, MessageBody: body } as unknown as Record<string, string>, 'json', 'AmazonSQS');
}

export async function receiveMessages(queueUrl: string, maxMessages = 10): Promise<{ messages: SqsMessage[] }> {
    const data = await awsRequest('sqs', 'ReceiveMessage', {
        QueueUrl: queueUrl,
        MaxNumberOfMessages: maxMessages,
        AttributeNames: ['All'],
    } as unknown as Record<string, string>, 'json', 'AmazonSQS') as { Messages?: { MessageId: string; ReceiptHandle: string; Body: string; Attributes?: Record<string, string> }[] };
    return {
        messages: (data.Messages ?? []).map((m) => ({
            messageId: m.MessageId,
            receiptHandle: m.ReceiptHandle,
            body: m.Body,
            attributes: m.Attributes ?? {},
        })),
    };
}

export async function deleteMessage(queueUrl: string, receiptHandle: string): Promise<void> {
    await awsRequest('sqs', 'DeleteMessage', { QueueUrl: queueUrl, ReceiptHandle: receiptHandle } as unknown as Record<string, string>, 'json', 'AmazonSQS');
}

export async function purgeQueue(queueUrl: string): Promise<void> {
    await awsRequest('sqs', 'PurgeQueue', { QueueUrl: queueUrl } as unknown as Record<string, string>, 'json', 'AmazonSQS');
}

// ---- DynamoDB ----

export interface DynamoTable {
    name: string;
}

export interface DynamoKeySchema {
    attributeName: string;
    keyType: 'HASH' | 'RANGE';
}

export interface DynamoTableDetail {
    name: string;
    status: string;
    itemCount: number;
    tableSizeBytes: number;
    keySchema: DynamoKeySchema[];
    creationDateTime: string;
}

export interface DynamoAttributeValue {
    S?: string;
    N?: string;
    B?: string;
    BOOL?: boolean;
    NULL?: boolean;
    L?: DynamoAttributeValue[];
    M?: Record<string, DynamoAttributeValue>;
    SS?: string[];
    NS?: string[];
    BS?: string[];
}

export async function listTables(): Promise<{ tables: DynamoTable[] }> {
    const data = await awsRequest('dynamodb', 'ListTables', {}, 'json', 'DynamoDB_20120810') as { TableNames?: string[] };
    return {
        tables: (data.TableNames ?? []).map((name) => ({ name })),
    };
}

export async function describeTable(name: string): Promise<DynamoTableDetail> {
    const data = await awsRequest('dynamodb', 'DescribeTable', { TableName: name } as unknown as Record<string, string>, 'json', 'DynamoDB_20120810') as {
        Table?: {
            TableName: string;
            TableStatus: string;
            ItemCount: number;
            TableSizeBytes: number;
            KeySchema: { AttributeName: string; KeyType: string }[];
            CreationDateTime: number;
        }
    };
    const t = data.Table ?? {} as NonNullable<typeof data.Table>;
    return {
        name: t?.TableName ?? name,
        status: t?.TableStatus ?? '',
        itemCount: t?.ItemCount ?? 0,
        tableSizeBytes: t?.TableSizeBytes ?? 0,
        keySchema: (t?.KeySchema ?? []).map((k) => ({
            attributeName: k.AttributeName,
            keyType: k.KeyType as 'HASH' | 'RANGE',
        })),
        creationDateTime: t?.CreationDateTime ? new Date(t.CreationDateTime * 1000).toISOString() : '',
    };
}

export async function createTable(
    name: string,
    partitionKey: string,
    partitionKeyType: 'S' | 'N' | 'B' = 'S',
    sortKey?: string,
    sortKeyType: 'S' | 'N' | 'B' = 'S'
): Promise<void> {
    const attributeDefinitions: { AttributeName: string; AttributeType: string }[] = [
        { AttributeName: partitionKey, AttributeType: partitionKeyType },
    ];
    const keySchema: { AttributeName: string; KeyType: string }[] = [
        { AttributeName: partitionKey, KeyType: 'HASH' },
    ];
    if (sortKey) {
        attributeDefinitions.push({ AttributeName: sortKey, AttributeType: sortKeyType });
        keySchema.push({ AttributeName: sortKey, KeyType: 'RANGE' });
    }
    await awsRequest('dynamodb', 'CreateTable', {
        TableName: name,
        AttributeDefinitions: attributeDefinitions,
        KeySchema: keySchema,
        BillingMode: 'PAY_PER_REQUEST',
    } as unknown as Record<string, string>, 'json', 'DynamoDB_20120810');
}

export async function deleteTable(name: string): Promise<void> {
    await awsRequest('dynamodb', 'DeleteTable', { TableName: name } as unknown as Record<string, string>, 'json', 'DynamoDB_20120810');
}

export async function scanTable(name: string, limit = 50): Promise<{ items: Record<string, DynamoAttributeValue>[]; count: number; scannedCount: number }> {
    const data = await awsRequest('dynamodb', 'Scan', {
        TableName: name,
        Limit: limit,
    } as unknown as Record<string, string>, 'json', 'DynamoDB_20120810') as {
        Items?: Record<string, DynamoAttributeValue>[];
        Count?: number;
        ScannedCount?: number;
    };
    return {
        items: data.Items ?? [],
        count: data.Count ?? 0,
        scannedCount: data.ScannedCount ?? 0,
    };
}

export async function putItem(tableName: string, item: Record<string, DynamoAttributeValue>): Promise<void> {
    await awsRequest('dynamodb', 'PutItem', {
        TableName: tableName,
        Item: item,
    } as unknown as Record<string, string>, 'json', 'DynamoDB_20120810');
}

export async function deleteItem(tableName: string, key: Record<string, DynamoAttributeValue>): Promise<void> {
    await awsRequest('dynamodb', 'DeleteItem', {
        TableName: tableName,
        Key: key,
    } as unknown as Record<string, string>, 'json', 'DynamoDB_20120810');
}

// ---- SNS ----

export interface SnsTopic {
    arn: string;
    name: string;
}

export async function listTopics(): Promise<{ topics: SnsTopic[] }> {
    const data = await awsRequest('sns', 'ListTopics', {}, 'json', 'AmazonSNS') as { Topics?: { TopicArn: string }[] };
    return {
        topics: (data.Topics ?? []).map((t) => ({
            arn: t.TopicArn,
            name: t.TopicArn.split(':').pop() ?? t.TopicArn,
        })),
    };
}

// ---- Lambda ----

export interface LambdaFunction {
    name: string;
    runtime: string;
    memory: number;
    handler: string;
    lastModified: string;
}

export async function listFunctions(): Promise<{ functions: LambdaFunction[] }> {
    const res = await fetch(`${ENDPOINT}/2015-03-31/functions`, {
        headers: {
            'Authorization': authHeader('lambda'),
            'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
        },
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const data = await res.json() as { Functions?: { FunctionName: string; Runtime: string; MemorySize: number; Handler: string; LastModified: string }[] };
    return {
        functions: (data.Functions ?? []).map((f) => ({
            name: f.FunctionName,
            runtime: f.Runtime,
            memory: f.MemorySize,
            handler: f.Handler,
            lastModified: f.LastModified,
        })),
    };
}

// ---- Secrets Manager ----

export interface Secret {
    name: string;
    arn: string;
    lastChanged: string;
}

export async function listSecrets(): Promise<{ secrets: Secret[] }> {
    const data = await awsRequest('secretsmanager', 'ListSecrets', {}, 'json', 'secretsmanager') as {
        SecretList?: { Name: string; ARN: string; LastChangedDate?: number }[]
    };
    return {
        secrets: (data.SecretList ?? []).map((s) => ({
            name: s.Name,
            arn: s.ARN,
            lastChanged: s.LastChangedDate
                ? new Date(s.LastChangedDate * 1000).toISOString()
                : '—',
        })),
    };
}

// ---- CloudWatch Logs ----

export interface LogGroup {
    name: string;
    retentionDays?: number;
    storedBytes: number;
}

export async function listLogGroups(): Promise<{ logGroups: LogGroup[] }> {
    const data = await awsRequest('logs', 'DescribeLogGroups', {}, 'json', 'Logs_20140328') as {
        logGroups?: { logGroupName: string; retentionInDays?: number; storedBytes?: number }[]
    };
    return {
        logGroups: (data.logGroups ?? []).map((g) => ({
            name: g.logGroupName,
            retentionDays: g.retentionInDays,
            storedBytes: g.storedBytes ?? 0,
        })),
    };
}

// ---- Step Functions ----

export interface StateMachine {
    name: string;
    arn: string;
    type: string;
    creationDate: string;
}

export async function listStateMachines(): Promise<{ stateMachines: StateMachine[] }> {
    const data = await awsRequest('states', 'ListStateMachines', {}, 'json', 'AWSStepFunctions') as {
        stateMachines?: { name: string; stateMachineArn: string; type: string; creationDate: number }[]
    };
    return {
        stateMachines: (data.stateMachines ?? []).map((m) => ({
            name: m.name,
            arn: m.stateMachineArn,
            type: m.type,
            creationDate: m.creationDate
                ? new Date(m.creationDate).toISOString()
                : '—',
        })),
    };
}
