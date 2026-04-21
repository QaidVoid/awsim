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

// ---- IAM ----

function xmlValue(xml: string, tag: string): string {
    const match = xml.match(new RegExp(`<${tag}>([^<]*)</${tag}>`));
    return match ? match[1] : '';
}

function xmlArray(xml: string, itemTag: string, fields: string[]): Record<string, string>[] {
    const items: Record<string, string>[] = [];
    const regex = new RegExp(`<${itemTag}>([\\s\\S]*?)</${itemTag}>`, 'g');
    let match;
    while ((match = regex.exec(xml)) !== null) {
        const item: Record<string, string> = {};
        for (const field of fields) {
            item[field] = xmlValue(match[1], field);
        }
        items.push(item);
    }
    return items;
}

async function iamRequest(action: string, params: Record<string, string> = {}): Promise<string> {
    const body = new URLSearchParams({ Action: action, Version: '2010-05-08', ...params });
    const res = await fetch(ENDPOINT, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/x-www-form-urlencoded',
            'Authorization': authHeader('iam'),
            'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
        },
        body: body.toString(),
    });
    return res.text();
}

export interface IamUser {
    userName: string;
    userId: string;
    arn: string;
    createDate: string;
}

export interface IamRole {
    roleName: string;
    roleId: string;
    arn: string;
}

export interface IamPolicy {
    policyName: string;
    arn: string;
    attachmentCount: string;
}

export interface IamGroup {
    groupName: string;
    groupId: string;
    arn: string;
}

export async function listUsers(): Promise<{ users: IamUser[] }> {
    const xml = await iamRequest('ListUsers');
    const raw = xmlArray(xml, 'member', ['UserName', 'UserId', 'Arn', 'CreateDate']);
    return {
        users: raw.map((u) => ({
            userName: u['UserName'] ?? '',
            userId: u['UserId'] ?? '',
            arn: u['Arn'] ?? '',
            createDate: u['CreateDate'] ?? '',
        })),
    };
}

export async function createUser(userName: string): Promise<void> {
    await iamRequest('CreateUser', { UserName: userName });
}

export async function deleteUser(userName: string): Promise<void> {
    await iamRequest('DeleteUser', { UserName: userName });
}

export async function listRoles(): Promise<{ roles: IamRole[] }> {
    const xml = await iamRequest('ListRoles');
    const raw = xmlArray(xml, 'member', ['RoleName', 'RoleId', 'Arn']);
    return {
        roles: raw.map((r) => ({
            roleName: r['RoleName'] ?? '',
            roleId: r['RoleId'] ?? '',
            arn: r['Arn'] ?? '',
        })),
    };
}

export async function createRole(roleName: string, assumeRolePolicy: string): Promise<void> {
    await iamRequest('CreateRole', { RoleName: roleName, AssumeRolePolicyDocument: assumeRolePolicy });
}

export async function deleteRole(roleName: string): Promise<void> {
    await iamRequest('DeleteRole', { RoleName: roleName });
}

export async function listPolicies(): Promise<{ policies: IamPolicy[] }> {
    const xml = await iamRequest('ListPolicies', { Scope: 'Local' });
    const raw = xmlArray(xml, 'member', ['PolicyName', 'Arn', 'AttachmentCount']);
    return {
        policies: raw.map((p) => ({
            policyName: p['PolicyName'] ?? '',
            arn: p['Arn'] ?? '',
            attachmentCount: p['AttachmentCount'] ?? '0',
        })),
    };
}

export async function listGroups(): Promise<{ groups: IamGroup[] }> {
    const xml = await iamRequest('ListGroups');
    const raw = xmlArray(xml, 'member', ['GroupName', 'GroupId', 'Arn']);
    return {
        groups: raw.map((g) => ({
            groupName: g['GroupName'] ?? '',
            groupId: g['GroupId'] ?? '',
            arn: g['Arn'] ?? '',
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

function lambdaHeaders(): Record<string, string> {
    return {
        'Authorization': authHeader('lambda'),
        'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
    };
}

export async function listFunctions(): Promise<{ functions: LambdaFunction[] }> {
    const res = await fetch(`${ENDPOINT}/2015-03-31/functions`, {
        headers: lambdaHeaders(),
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

export async function createFunction(
    name: string,
    runtime: string,
    handler: string,
    roleArn: string,
    code: string
): Promise<void> {
    const res = await fetch(`${ENDPOINT}/2015-03-31/functions`, {
        method: 'POST',
        headers: {
            ...lambdaHeaders(),
            'Content-Type': 'application/json',
        },
        body: JSON.stringify({
            FunctionName: name,
            Runtime: runtime,
            Handler: handler,
            Role: roleArn,
            Code: { ZipFile: code },
        }),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}

export async function deleteFunction(name: string): Promise<void> {
    const res = await fetch(`${ENDPOINT}/2015-03-31/functions/${encodeURIComponent(name)}`, {
        method: 'DELETE',
        headers: lambdaHeaders(),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}

export async function invokeFunction(name: string, payload: string): Promise<string> {
    const res = await fetch(`${ENDPOINT}/2015-03-31/functions/${encodeURIComponent(name)}/invocations`, {
        method: 'POST',
        headers: {
            ...lambdaHeaders(),
            'Content-Type': 'application/json',
        },
        body: payload,
    });
    const text = await res.text();
    return text;
}

// ---- Cognito ----

async function cognitoRequest(action: string, body: unknown): Promise<unknown> {
    const res = await fetch(ENDPOINT, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/x-amz-json-1.1',
            'X-Amz-Target': `AWSCognitoIdentityProviderService.${action}`,
            'Authorization': authHeader('cognito-idp'),
            'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
        },
        body: JSON.stringify(body),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
    return res.json();
}

export interface CognitoUserPool {
    id: string;
    name: string;
    status: string;
    creationDate: string;
}

export interface CognitoUser {
    username: string;
    status: string;
    createDate: string;
}

export async function listUserPools(): Promise<{ userPools: CognitoUserPool[] }> {
    const data = await cognitoRequest('ListUserPools', { MaxResults: 60 }) as {
        UserPools?: { Id: string; Name: string; Status?: string; CreationDate?: number }[]
    };
    return {
        userPools: (data.UserPools ?? []).map((p) => ({
            id: p.Id,
            name: p.Name,
            status: p.Status ?? 'ACTIVE',
            creationDate: p.CreationDate ? new Date(p.CreationDate * 1000).toISOString() : '',
        })),
    };
}

export async function createUserPool(poolName: string): Promise<void> {
    await cognitoRequest('CreateUserPool', { PoolName: poolName });
}

export async function deleteUserPool(userPoolId: string): Promise<void> {
    await cognitoRequest('DeleteUserPool', { UserPoolId: userPoolId });
}

export async function listCognitoUsers(userPoolId: string): Promise<{ users: CognitoUser[] }> {
    const data = await cognitoRequest('ListUsers', { UserPoolId: userPoolId }) as {
        Users?: { Username: string; UserStatus?: string; UserCreateDate?: number }[]
    };
    return {
        users: (data.Users ?? []).map((u) => ({
            username: u.Username,
            status: u.UserStatus ?? '',
            createDate: u.UserCreateDate ? new Date(u.UserCreateDate * 1000).toISOString() : '',
        })),
    };
}

export async function adminCreateUser(userPoolId: string, username: string): Promise<void> {
    await cognitoRequest('AdminCreateUser', { UserPoolId: userPoolId, Username: username });
}

export async function adminDeleteUser(userPoolId: string, username: string): Promise<void> {
    await cognitoRequest('AdminDeleteUser', { UserPoolId: userPoolId, Username: username });
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

// ---- EventBridge ----

export interface EventBus {
    name: string;
    arn: string;
}

export interface EventRule {
    name: string;
    arn: string;
    state: string;
    eventPattern?: string;
    description?: string;
}

export async function listEventBuses(): Promise<{ eventBuses: EventBus[] }> {
    const data = await awsRequest('events', 'ListEventBuses', {}, 'json', 'AWSEvents') as {
        EventBuses?: { Name: string; Arn: string }[]
    };
    return {
        eventBuses: (data.EventBuses ?? []).map((b) => ({
            name: b.Name,
            arn: b.Arn,
        })),
    };
}

export async function listRules(busName?: string): Promise<{ rules: EventRule[] }> {
    const params: Record<string, unknown> = {};
    if (busName) params['EventBusName'] = busName;
    const data = await awsRequest('events', 'ListRules', params as unknown as Record<string, string>, 'json', 'AWSEvents') as {
        Rules?: { Name: string; Arn: string; State: string; EventPattern?: string; Description?: string }[]
    };
    return {
        rules: (data.Rules ?? []).map((r) => ({
            name: r.Name,
            arn: r.Arn,
            state: r.State,
            eventPattern: r.EventPattern,
            description: r.Description,
        })),
    };
}

export async function putRule(name: string, busName: string, eventPattern: string): Promise<void> {
    await awsRequest('events', 'PutRule', {
        Name: name,
        EventBusName: busName,
        EventPattern: eventPattern,
        State: 'ENABLED',
    } as unknown as Record<string, string>, 'json', 'AWSEvents');
}

export async function deleteRule(name: string, busName: string): Promise<void> {
    await awsRequest('events', 'DeleteRule', {
        Name: name,
        EventBusName: busName,
    } as unknown as Record<string, string>, 'json', 'AWSEvents');
}

export async function putEvents(entries: { Source: string; DetailType: string; Detail: string; EventBusName?: string }[]): Promise<void> {
    await awsRequest('events', 'PutEvents', {
        Entries: entries,
    } as unknown as Record<string, string>, 'json', 'AWSEvents');
}

// ---- KMS ----

export interface KmsKey {
    keyId: string;
    keyArn: string;
}

export interface KmsKeyDetail {
    keyId: string;
    keyArn: string;
    description: string;
    keyState: string;
    creationDate: string;
}

export interface KmsAlias {
    aliasName: string;
    aliasArn: string;
    targetKeyId: string;
}

export async function listKeys(): Promise<{ keys: KmsKey[] }> {
    const data = await awsRequest('kms', 'ListKeys', {}, 'json', 'TrentService') as {
        Keys?: { KeyId: string; KeyArn: string }[]
    };
    return {
        keys: (data.Keys ?? []).map((k) => ({
            keyId: k.KeyId,
            keyArn: k.KeyArn,
        })),
    };
}

export async function describeKey(keyId: string): Promise<KmsKeyDetail> {
    const data = await awsRequest('kms', 'DescribeKey', { KeyId: keyId } as unknown as Record<string, string>, 'json', 'TrentService') as {
        KeyMetadata?: { KeyId: string; Arn: string; Description?: string; KeyState: string; CreationDate: number }
    };
    const k = data.KeyMetadata ?? {} as NonNullable<typeof data.KeyMetadata>;
    return {
        keyId: k?.KeyId ?? keyId,
        keyArn: k?.Arn ?? '',
        description: k?.Description ?? '',
        keyState: k?.KeyState ?? '',
        creationDate: k?.CreationDate ? new Date(k.CreationDate * 1000).toISOString() : '',
    };
}

export async function createKey(description?: string): Promise<{ keyId: string }> {
    const params: Record<string, unknown> = {};
    if (description) params['Description'] = description;
    const data = await awsRequest('kms', 'CreateKey', params as unknown as Record<string, string>, 'json', 'TrentService') as {
        KeyMetadata?: { KeyId: string }
    };
    return { keyId: data.KeyMetadata?.KeyId ?? '' };
}

export async function listAliases(): Promise<{ aliases: KmsAlias[] }> {
    const data = await awsRequest('kms', 'ListAliases', {}, 'json', 'TrentService') as {
        Aliases?: { AliasName: string; AliasArn: string; TargetKeyId?: string }[]
    };
    return {
        aliases: (data.Aliases ?? []).map((a) => ({
            aliasName: a.AliasName,
            aliasArn: a.AliasArn,
            targetKeyId: a.TargetKeyId ?? '',
        })),
    };
}

export async function createAlias(aliasName: string, targetKeyId: string): Promise<void> {
    await awsRequest('kms', 'CreateAlias', {
        AliasName: aliasName,
        TargetKeyId: targetKeyId,
    } as unknown as Record<string, string>, 'json', 'TrentService');
}

export async function kmsEncrypt(keyId: string, plaintext: string): Promise<{ ciphertextBlob: string }> {
    // plaintext must be base64-encoded for KMS
    const encoded = btoa(plaintext);
    const data = await awsRequest('kms', 'Encrypt', {
        KeyId: keyId,
        Plaintext: encoded,
    } as unknown as Record<string, string>, 'json', 'TrentService') as {
        CiphertextBlob: string
    };
    return { ciphertextBlob: data.CiphertextBlob ?? '' };
}

export async function kmsDecrypt(ciphertextBlob: string): Promise<{ plaintext: string }> {
    const data = await awsRequest('kms', 'Decrypt', {
        CiphertextBlob: ciphertextBlob,
    } as unknown as Record<string, string>, 'json', 'TrentService') as {
        Plaintext: string
    };
    // Plaintext is returned as base64
    return { plaintext: atob(data.Plaintext ?? '') };
}

// ---- SSM Parameter Store ----

export interface SsmParameter {
    name: string;
    type: string;
    version: number;
    lastModifiedDate: string;
}

export interface SsmParameterValue {
    name: string;
    type: string;
    value: string;
    version: number;
}

export async function listParameters(): Promise<{ parameters: SsmParameter[] }> {
    const data = await awsRequest('ssm', 'DescribeParameters', {}, 'json', 'AmazonSSM') as {
        Parameters?: { Name: string; Type: string; Version?: number; LastModifiedDate?: number }[]
    };
    return {
        parameters: (data.Parameters ?? []).map((p) => ({
            name: p.Name,
            type: p.Type,
            version: p.Version ?? 1,
            lastModifiedDate: p.LastModifiedDate
                ? new Date(p.LastModifiedDate * 1000).toISOString()
                : '',
        })),
    };
}

export async function getParameter(name: string): Promise<SsmParameterValue> {
    const data = await awsRequest('ssm', 'GetParameter', {
        Name: name,
        WithDecryption: true,
    } as unknown as Record<string, string>, 'json', 'AmazonSSM') as {
        Parameter?: { Name: string; Type: string; Value: string; Version?: number }
    };
    const p = data.Parameter ?? {} as NonNullable<typeof data.Parameter>;
    return {
        name: p?.Name ?? name,
        type: p?.Type ?? '',
        value: p?.Value ?? '',
        version: p?.Version ?? 1,
    };
}

export async function putParameter(name: string, value: string, type: string): Promise<void> {
    await awsRequest('ssm', 'PutParameter', {
        Name: name,
        Value: value,
        Type: type,
        Overwrite: true,
    } as unknown as Record<string, string>, 'json', 'AmazonSSM');
}

export async function deleteParameter(name: string): Promise<void> {
    await awsRequest('ssm', 'DeleteParameter', {
        Name: name,
    } as unknown as Record<string, string>, 'json', 'AmazonSSM');
}

// ---- SES v2 ----

export interface SesIdentity {
    emailIdentity: string;
    verificationStatus: string;
    identityType: string;
}

export interface SesTemplate {
    templateName: string;
    createdTimestamp: string;
}

function sesHeaders(): Record<string, string> {
    return {
        'Authorization': authHeader('ses'),
        'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
        'Content-Type': 'application/json',
    };
}

export async function listEmailIdentities(): Promise<{ identities: SesIdentity[] }> {
    const res = await fetch(`${ENDPOINT}/v2/email/identities`, {
        headers: sesHeaders(),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
    const data = await res.json() as {
        EmailIdentities?: { IdentityName: string; VerificationStatus?: string; IdentityType?: string }[]
    };
    return {
        identities: (data.EmailIdentities ?? []).map((i) => ({
            emailIdentity: i.IdentityName,
            verificationStatus: i.VerificationStatus ?? 'VERIFIED',
            identityType: i.IdentityType ?? 'EMAIL_ADDRESS',
        })),
    };
}

export async function createEmailIdentity(emailIdentity: string): Promise<void> {
    const res = await fetch(`${ENDPOINT}/v2/email/identities`, {
        method: 'POST',
        headers: sesHeaders(),
        body: JSON.stringify({ EmailIdentity: emailIdentity }),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}

export async function deleteEmailIdentity(emailIdentity: string): Promise<void> {
    const res = await fetch(`${ENDPOINT}/v2/email/identities/${encodeURIComponent(emailIdentity)}`, {
        method: 'DELETE',
        headers: sesHeaders(),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}

export async function listEmailTemplates(): Promise<{ templates: SesTemplate[] }> {
    const res = await fetch(`${ENDPOINT}/v2/email/templates`, {
        headers: sesHeaders(),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
    const data = await res.json() as {
        TemplatesMetadata?: { TemplateName: string; CreatedTimestamp?: string }[]
    };
    return {
        templates: (data.TemplatesMetadata ?? []).map((t) => ({
            templateName: t.TemplateName,
            createdTimestamp: t.CreatedTimestamp ?? '',
        })),
    };
}

export async function createEmailTemplate(
    templateName: string,
    subject: string,
    htmlBody: string
): Promise<void> {
    const res = await fetch(`${ENDPOINT}/v2/email/templates`, {
        method: 'POST',
        headers: sesHeaders(),
        body: JSON.stringify({
            TemplateName: templateName,
            TemplateContent: {
                Subject: subject,
                Html: htmlBody,
            },
        }),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}

export async function deleteEmailTemplate(templateName: string): Promise<void> {
    const res = await fetch(`${ENDPOINT}/v2/email/templates/${encodeURIComponent(templateName)}`, {
        method: 'DELETE',
        headers: sesHeaders(),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}

// ---- Step Functions (extended) ----

export interface SfnExecution {
    name: string;
    arn: string;
    status: string;
    startDate: string;
    stopDate?: string;
}

export async function createStateMachine(name: string, definition: string): Promise<void> {
    await awsRequest('states', 'CreateStateMachine', {
        name,
        definition,
        roleArn: 'arn:aws:iam::000000000000:role/exec',
        type: 'STANDARD',
    } as unknown as Record<string, string>, 'json', 'AWSStepFunctions');
}

export async function deleteStateMachine(arn: string): Promise<void> {
    await awsRequest('states', 'DeleteStateMachine', {
        stateMachineArn: arn,
    } as unknown as Record<string, string>, 'json', 'AWSStepFunctions');
}

export async function listExecutions(stateMachineArn: string): Promise<{ executions: SfnExecution[] }> {
    const data = await awsRequest('states', 'ListExecutions', {
        stateMachineArn,
    } as unknown as Record<string, string>, 'json', 'AWSStepFunctions') as {
        executions?: { name: string; executionArn: string; status: string; startDate: number; stopDate?: number }[]
    };
    return {
        executions: (data.executions ?? []).map((e) => ({
            name: e.name,
            arn: e.executionArn,
            status: e.status,
            startDate: e.startDate ? new Date(e.startDate).toISOString() : '—',
            stopDate: e.stopDate ? new Date(e.stopDate).toISOString() : undefined,
        })),
    };
}

export async function startExecution(stateMachineArn: string, input: string): Promise<void> {
    await awsRequest('states', 'StartExecution', {
        stateMachineArn,
        input,
    } as unknown as Record<string, string>, 'json', 'AWSStepFunctions');
}

// ---- EC2 ----

async function ec2Request(action: string, params: Record<string, string> = {}): Promise<string> {
    const body = new URLSearchParams({ Action: action, Version: '2016-11-15', ...params });
    const res = await fetch(ENDPOINT, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/x-www-form-urlencoded',
            'Authorization': authHeader('ec2'),
            'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
        },
        body: body.toString(),
    });
    return res.text();
}

export interface Ec2Vpc {
    vpcId: string;
    cidrBlock: string;
    state: string;
    isDefault: string;
}

export interface Ec2Subnet {
    subnetId: string;
    vpcId: string;
    cidrBlock: string;
    availabilityZone: string;
    availableIpAddressCount: string;
}

export interface Ec2SecurityGroup {
    groupId: string;
    groupName: string;
    description: string;
    vpcId: string;
}

export async function describeVpcs(): Promise<{ vpcs: Ec2Vpc[] }> {
    const xml = await ec2Request('DescribeVpcs');
    const raw = xmlArray(xml, 'item', ['vpcId', 'cidrBlock', 'state', 'isDefault']);
    return {
        vpcs: raw.map((v) => ({
            vpcId: v['vpcId'] ?? '',
            cidrBlock: v['cidrBlock'] ?? '',
            state: v['state'] ?? '',
            isDefault: v['isDefault'] ?? '',
        })),
    };
}

export async function describeSubnets(): Promise<{ subnets: Ec2Subnet[] }> {
    const xml = await ec2Request('DescribeSubnets');
    const raw = xmlArray(xml, 'item', ['subnetId', 'vpcId', 'cidrBlock', 'availabilityZone', 'availableIpAddressCount']);
    return {
        subnets: raw.map((s) => ({
            subnetId: s['subnetId'] ?? '',
            vpcId: s['vpcId'] ?? '',
            cidrBlock: s['cidrBlock'] ?? '',
            availabilityZone: s['availabilityZone'] ?? '',
            availableIpAddressCount: s['availableIpAddressCount'] ?? '',
        })),
    };
}

export async function describeSecurityGroups(): Promise<{ securityGroups: Ec2SecurityGroup[] }> {
    const xml = await ec2Request('DescribeSecurityGroups');
    const raw = xmlArray(xml, 'item', ['groupId', 'groupName', 'groupDescription', 'vpcId']);
    return {
        securityGroups: raw.map((g) => ({
            groupId: g['groupId'] ?? '',
            groupName: g['groupName'] ?? '',
            description: g['groupDescription'] ?? '',
            vpcId: g['vpcId'] ?? '',
        })),
    };
}

export async function createVpc(cidrBlock: string): Promise<void> {
    await ec2Request('CreateVpc', { CidrBlock: cidrBlock });
}

export async function deleteVpc(vpcId: string): Promise<void> {
    await ec2Request('DeleteVpc', { VpcId: vpcId });
}

export async function createSecurityGroup(name: string, desc: string, vpcId: string): Promise<void> {
    await ec2Request('CreateSecurityGroup', { GroupName: name, Description: desc, VpcId: vpcId });
}

export async function deleteSecurityGroup(groupId: string): Promise<void> {
    await ec2Request('DeleteSecurityGroup', { GroupId: groupId });
}

// ---- ECS ----

export interface EcsCluster {
    clusterArn: string;
    clusterName: string;
    status: string;
}

export async function listClusters(): Promise<{ clusterArns: string[] }> {
    const data = await awsRequest('ecs', 'ListClusters', {}, 'json', 'AmazonEC2ContainerServiceV20141113') as {
        clusterArns?: string[]
    };
    return { clusterArns: data.clusterArns ?? [] };
}

export async function describeClusters(clusterArns: string[]): Promise<{ clusters: EcsCluster[] }> {
    const data = await awsRequest('ecs', 'DescribeClusters', {
        clusters: clusterArns,
    } as unknown as Record<string, string>, 'json', 'AmazonEC2ContainerServiceV20141113') as {
        clusters?: { clusterArn: string; clusterName: string; status: string }[]
    };
    return {
        clusters: (data.clusters ?? []).map((c) => ({
            clusterArn: c.clusterArn,
            clusterName: c.clusterName,
            status: c.status,
        })),
    };
}

export async function createCluster(name: string): Promise<void> {
    await awsRequest('ecs', 'CreateCluster', {
        clusterName: name,
    } as unknown as Record<string, string>, 'json', 'AmazonEC2ContainerServiceV20141113');
}

export async function deleteCluster(cluster: string): Promise<void> {
    await awsRequest('ecs', 'DeleteCluster', {
        cluster,
    } as unknown as Record<string, string>, 'json', 'AmazonEC2ContainerServiceV20141113');
}

export async function listTaskDefinitions(): Promise<{ taskDefinitionArns: string[] }> {
    const data = await awsRequest('ecs', 'ListTaskDefinitions', {}, 'json', 'AmazonEC2ContainerServiceV20141113') as {
        taskDefinitionArns?: string[]
    };
    return { taskDefinitionArns: data.taskDefinitionArns ?? [] };
}

export async function listServices(cluster: string): Promise<{ serviceArns: string[] }> {
    const data = await awsRequest('ecs', 'ListServices', {
        cluster,
    } as unknown as Record<string, string>, 'json', 'AmazonEC2ContainerServiceV20141113') as {
        serviceArns?: string[]
    };
    return { serviceArns: data.serviceArns ?? [] };
}

// ---- ECR ----

export interface EcrRepository {
    repositoryName: string;
    repositoryUri: string;
    repositoryArn: string;
    createdAt: string;
}

export async function listRepositories(): Promise<{ repositories: EcrRepository[] }> {
    const data = await awsRequest('ecr', 'DescribeRepositories', {}, 'json', 'AmazonEC2ContainerRegistry_V20150921') as {
        repositories?: { repositoryName: string; repositoryUri: string; repositoryArn: string; createdAt?: number }[]
    };
    return {
        repositories: (data.repositories ?? []).map((r) => ({
            repositoryName: r.repositoryName,
            repositoryUri: r.repositoryUri,
            repositoryArn: r.repositoryArn,
            createdAt: r.createdAt ? new Date(r.createdAt * 1000).toISOString() : '—',
        })),
    };
}

export async function createRepository(name: string): Promise<void> {
    await awsRequest('ecr', 'CreateRepository', {
        repositoryName: name,
    } as unknown as Record<string, string>, 'json', 'AmazonEC2ContainerRegistry_V20150921');
}

export async function deleteRepository(name: string): Promise<void> {
    await awsRequest('ecr', 'DeleteRepository', {
        repositoryName: name,
        force: true,
    } as unknown as Record<string, string>, 'json', 'AmazonEC2ContainerRegistry_V20150921');
}

// ---- CloudFormation ----

async function cfRequest(action: string, params: Record<string, string> = {}): Promise<string> {
    const body = new URLSearchParams({ Action: action, Version: '2010-05-15', ...params });
    const res = await fetch(ENDPOINT, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/x-www-form-urlencoded',
            'Authorization': authHeader('cloudformation'),
            'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
        },
        body: body.toString(),
    });
    return res.text();
}

export interface CfStack {
    stackName: string;
    stackId: string;
    stackStatus: string;
    creationTime: string;
    description: string;
}

export interface CfStackResource {
    logicalResourceId: string;
    physicalResourceId: string;
    resourceType: string;
    resourceStatus: string;
}

export interface CfStackEvent {
    eventId: string;
    stackName: string;
    logicalResourceId: string;
    resourceType: string;
    resourceStatus: string;
    timestamp: string;
}

export async function listStacks(): Promise<{ stacks: CfStack[] }> {
    const xml = await cfRequest('ListStacks');
    const raw = xmlArray(xml, 'member', ['StackName', 'StackId', 'StackStatus', 'CreationTime', 'TemplateDescription']);
    return {
        stacks: raw.map((s) => ({
            stackName: s['StackName'] ?? '',
            stackId: s['StackId'] ?? '',
            stackStatus: s['StackStatus'] ?? '',
            creationTime: s['CreationTime'] ?? '',
            description: s['TemplateDescription'] ?? '',
        })),
    };
}

export async function describeStacks(stackName?: string): Promise<{ stacks: CfStack[] }> {
    const params: Record<string, string> = stackName ? { StackName: stackName } : {};
    const xml = await cfRequest('DescribeStacks', params);
    const raw = xmlArray(xml, 'member', ['StackName', 'StackId', 'StackStatus', 'CreationTime', 'Description']);
    return {
        stacks: raw.map((s) => ({
            stackName: s['StackName'] ?? '',
            stackId: s['StackId'] ?? '',
            stackStatus: s['StackStatus'] ?? '',
            creationTime: s['CreationTime'] ?? '',
            description: s['Description'] ?? '',
        })),
    };
}

export async function createStack(name: string, templateBody: string): Promise<void> {
    await cfRequest('CreateStack', { StackName: name, TemplateBody: templateBody });
}

export async function deleteStack(name: string): Promise<void> {
    await cfRequest('DeleteStack', { StackName: name });
}

export async function describeStackResources(stackName: string): Promise<{ resources: CfStackResource[] }> {
    const xml = await cfRequest('DescribeStackResources', { StackName: stackName });
    const raw = xmlArray(xml, 'member', ['LogicalResourceId', 'PhysicalResourceId', 'ResourceType', 'ResourceStatus']);
    return {
        resources: raw.map((r) => ({
            logicalResourceId: r['LogicalResourceId'] ?? '',
            physicalResourceId: r['PhysicalResourceId'] ?? '',
            resourceType: r['ResourceType'] ?? '',
            resourceStatus: r['ResourceStatus'] ?? '',
        })),
    };
}

export async function describeStackEvents(stackName: string): Promise<{ events: CfStackEvent[] }> {
    const xml = await cfRequest('DescribeStackEvents', { StackName: stackName });
    const raw = xmlArray(xml, 'member', ['EventId', 'StackName', 'LogicalResourceId', 'ResourceType', 'ResourceStatus', 'Timestamp']);
    return {
        events: raw.map((e) => ({
            eventId: e['EventId'] ?? '',
            stackName: e['StackName'] ?? '',
            logicalResourceId: e['LogicalResourceId'] ?? '',
            resourceType: e['ResourceType'] ?? '',
            resourceStatus: e['ResourceStatus'] ?? '',
            timestamp: e['Timestamp'] ?? '',
        })),
    };
}

// ---- Kinesis ----

export interface KinesisStream {
    streamName: string;
    streamStatus: string;
    shardCount: number;
    retentionPeriodHours: number;
}

export interface KinesisShard {
    shardId: string;
    startingSequenceNumber: string;
    endingSequenceNumber?: string;
}

export async function listStreams(): Promise<{ streamNames: string[] }> {
    const data = await awsRequest('kinesis', 'ListStreams', {}, 'json', 'Kinesis_20131202') as {
        StreamNames?: string[]
    };
    return { streamNames: data.StreamNames ?? [] };
}

export async function describeStream(name: string): Promise<{ stream: KinesisStream; shards: KinesisShard[] }> {
    const data = await awsRequest('kinesis', 'DescribeStream', {
        StreamName: name,
    } as unknown as Record<string, string>, 'json', 'Kinesis_20131202') as {
        StreamDescription?: {
            StreamName: string;
            StreamStatus: string;
            RetentionPeriodHours: number;
            Shards: { ShardId: string; SequenceNumberRange: { StartingSequenceNumber: string; EndingSequenceNumber?: string } }[]
        }
    };
    const desc = data.StreamDescription;
    return {
        stream: {
            streamName: desc?.StreamName ?? name,
            streamStatus: desc?.StreamStatus ?? '',
            shardCount: desc?.Shards?.length ?? 0,
            retentionPeriodHours: desc?.RetentionPeriodHours ?? 24,
        },
        shards: (desc?.Shards ?? []).map((s) => ({
            shardId: s.ShardId,
            startingSequenceNumber: s.SequenceNumberRange.StartingSequenceNumber,
            endingSequenceNumber: s.SequenceNumberRange.EndingSequenceNumber,
        })),
    };
}

export async function createStream(name: string, shardCount: number): Promise<void> {
    await awsRequest('kinesis', 'CreateStream', {
        StreamName: name,
        ShardCount: shardCount,
    } as unknown as Record<string, string>, 'json', 'Kinesis_20131202');
}

export async function deleteStream(name: string): Promise<void> {
    await awsRequest('kinesis', 'DeleteStream', {
        StreamName: name,
    } as unknown as Record<string, string>, 'json', 'Kinesis_20131202');
}
