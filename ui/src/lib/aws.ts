const ENDPOINT = 'http://localhost:4566';

const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, '');

function authHeader(service: string): string {
    return `AWS4-HMAC-SHA256 Credential=test/${FAKE_DATE}/us-east-1/${service}/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

// ---- Request Log ----

export interface RequestLogEntry {
    id: number;
    timestamp: string;
    method: string;
    service: string;
    operation: string;
    status: number;
    duration: number;
}

let requestLog: RequestLogEntry[] = [];
let logId = 0;

export function getRequestLog(): RequestLogEntry[] {
    return [...requestLog].reverse();
}

export function clearRequestLog(): void {
    requestLog = [];
    logId = 0;
}

async function loggedFetch(
    service: string,
    operation: string,
    method: string,
    input: RequestInfo | URL,
    init?: RequestInit
): Promise<Response> {
    const start = Date.now();
    let status = 0;
    try {
        const res = await fetch(input, init);
        status = res.status;
        return res;
    } finally {
        const duration = Date.now() - start;
        requestLog.push({
            id: ++logId,
            timestamp: new Date().toISOString(),
            method,
            service,
            operation,
            status,
            duration,
        });
        if (requestLog.length > 500) requestLog.shift();
    }
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
        const res = await loggedFetch(service, action, 'POST', ENDPOINT, {
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
        const res = await loggedFetch(service, action, 'POST', ENDPOINT, {
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
    const res = await loggedFetch('s3', 'ListBuckets', 'GET', `${ENDPOINT}/`, {
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
    const res = await loggedFetch('s3', 'CreateBucket', 'PUT', `${ENDPOINT}/${encodeURIComponent(name)}`, {
        method: 'PUT',
        headers: s3Headers(),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}

export async function deleteBucket(name: string): Promise<void> {
    const res = await loggedFetch('s3', 'DeleteBucket', 'DELETE', `${ENDPOINT}/${encodeURIComponent(name)}`, {
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

    const res = await loggedFetch('s3', 'ListObjectsV2', 'GET', `${ENDPOINT}/${encodeURIComponent(bucket)}?${params.toString()}`, {
        headers: s3Headers(),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
    const text = await res.text();
    return parseXmlListObjects(text);
}

export async function deleteObject(bucket: string, key: string): Promise<void> {
    const res = await loggedFetch('s3', 'DeleteObject', 'DELETE', `${ENDPOINT}/${encodeURIComponent(bucket)}/${key.split('/').map(encodeURIComponent).join('/')}`, {
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
    const res = await loggedFetch('iam', action, 'POST', ENDPOINT, {
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

export async function createRole(roleName: string, assumeRolePolicy: string, description?: string): Promise<void> {
    const params: Record<string, string> = { RoleName: roleName, AssumeRolePolicyDocument: assumeRolePolicy };
    if (description) params['Description'] = description;
    await iamRequest('CreateRole', params);
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

// Extended IAM functions

export async function iamGetUser(userName: string): Promise<IamUser> {
    const xml = await iamRequest('GetUser', { UserName: userName });
    return {
        userName: xmlValue(xml, 'UserName'),
        userId: xmlValue(xml, 'UserId'),
        arn: xmlValue(xml, 'Arn'),
        createDate: xmlValue(xml, 'CreateDate'),
    };
}

export async function iamListGroupsForUser(userName: string): Promise<{ groups: IamGroup[] }> {
    const xml = await iamRequest('ListGroupsForUser', { UserName: userName });
    const raw = xmlArray(xml, 'member', ['GroupName', 'GroupId', 'Arn']);
    return {
        groups: raw.map((g) => ({
            groupName: g['GroupName'] ?? '',
            groupId: g['GroupId'] ?? '',
            arn: g['Arn'] ?? '',
        })),
    };
}

export async function iamListUserPolicies(userName: string): Promise<{ policyNames: string[] }> {
    const xml = await iamRequest('ListUserPolicies', { UserName: userName });
    const names: string[] = [];
    const regex = /<member>([^<]+)<\/member>/g;
    let m;
    while ((m = regex.exec(xml)) !== null) names.push(m[1]);
    return { policyNames: names };
}

export interface IamAttachedPolicy {
    policyName: string;
    policyArn: string;
}

export async function iamListAttachedUserPolicies(userName: string): Promise<{ policies: IamAttachedPolicy[] }> {
    const xml = await iamRequest('ListAttachedUserPolicies', { UserName: userName });
    const raw = xmlArray(xml, 'member', ['PolicyName', 'PolicyArn']);
    return {
        policies: raw.map((p) => ({
            policyName: p['PolicyName'] ?? '',
            policyArn: p['PolicyArn'] ?? '',
        })),
    };
}

export interface IamAccessKey {
    accessKeyId: string;
    status: string;
    createDate: string;
}

export async function iamListAccessKeys(userName: string): Promise<{ accessKeys: IamAccessKey[] }> {
    const xml = await iamRequest('ListAccessKeys', { UserName: userName });
    const raw = xmlArray(xml, 'member', ['AccessKeyId', 'Status', 'CreateDate']);
    return {
        accessKeys: raw.map((k) => ({
            accessKeyId: k['AccessKeyId'] ?? '',
            status: k['Status'] ?? '',
            createDate: k['CreateDate'] ?? '',
        })),
    };
}

export async function iamCreateAccessKey(userName: string): Promise<{ accessKeyId: string; secretAccessKey: string }> {
    const xml = await iamRequest('CreateAccessKey', { UserName: userName });
    return {
        accessKeyId: xmlValue(xml, 'AccessKeyId'),
        secretAccessKey: xmlValue(xml, 'SecretAccessKey'),
    };
}

export async function iamDeleteAccessKey(userName: string, accessKeyId: string): Promise<void> {
    await iamRequest('DeleteAccessKey', { UserName: userName, AccessKeyId: accessKeyId });
}

export interface IamTag {
    key: string;
    value: string;
}

export async function iamListUserTags(userName: string): Promise<{ tags: IamTag[] }> {
    const xml = await iamRequest('ListUserTags', { UserName: userName });
    const raw = xmlArray(xml, 'member', ['Key', 'Value']);
    return {
        tags: raw.map((t) => ({
            key: t['Key'] ?? '',
            value: t['Value'] ?? '',
        })),
    };
}

export async function iamTagUser(userName: string, tags: IamTag[]): Promise<void> {
    const params: Record<string, string> = { UserName: userName };
    tags.forEach((tag, i) => {
        params[`Tags.member.${i + 1}.Key`] = tag.key;
        params[`Tags.member.${i + 1}.Value`] = tag.value;
    });
    await iamRequest('TagUser', params);
}

export async function iamUntagUser(userName: string, tagKeys: string[]): Promise<void> {
    const params: Record<string, string> = { UserName: userName };
    tagKeys.forEach((key, i) => {
        params[`TagKeys.member.${i + 1}`] = key;
    });
    await iamRequest('UntagUser', params);
}

export async function iamGetRole(roleName: string): Promise<IamRole & { assumeRolePolicyDocument: string; description?: string; createDate?: string }> {
    const xml = await iamRequest('GetRole', { RoleName: roleName });
    const doc = xmlValue(xml, 'AssumeRolePolicyDocument');
    return {
        roleName: xmlValue(xml, 'RoleName'),
        roleId: xmlValue(xml, 'RoleId'),
        arn: xmlValue(xml, 'Arn'),
        assumeRolePolicyDocument: doc ? decodeURIComponent(doc) : '',
        description: xmlValue(xml, 'Description') || undefined,
        createDate: xmlValue(xml, 'CreateDate') || undefined,
    };
}

export async function iamListRolePolicies(roleName: string): Promise<{ policyNames: string[] }> {
    const xml = await iamRequest('ListRolePolicies', { RoleName: roleName });
    const names: string[] = [];
    const regex = /<member>([^<]+)<\/member>/g;
    let m;
    while ((m = regex.exec(xml)) !== null) names.push(m[1]);
    return { policyNames: names };
}

export async function iamListAttachedRolePolicies(roleName: string): Promise<{ policies: IamAttachedPolicy[] }> {
    const xml = await iamRequest('ListAttachedRolePolicies', { RoleName: roleName });
    const raw = xmlArray(xml, 'member', ['PolicyName', 'PolicyArn']);
    return {
        policies: raw.map((p) => ({
            policyName: p['PolicyName'] ?? '',
            policyArn: p['PolicyArn'] ?? '',
        })),
    };
}

export async function iamListRoleTags(roleName: string): Promise<{ tags: IamTag[] }> {
    const xml = await iamRequest('ListRoleTags', { RoleName: roleName });
    const raw = xmlArray(xml, 'member', ['Key', 'Value']);
    return {
        tags: raw.map((t) => ({
            key: t['Key'] ?? '',
            value: t['Value'] ?? '',
        })),
    };
}

export async function iamUpdateAssumeRolePolicy(roleName: string, policyDocument: string): Promise<void> {
    await iamRequest('UpdateAssumeRolePolicy', { RoleName: roleName, PolicyDocument: policyDocument });
}

export async function iamGetPolicy(arn: string): Promise<IamPolicy & { defaultVersionId: string; description?: string; createDate?: string }> {
    const xml = await iamRequest('GetPolicy', { PolicyArn: arn });
    return {
        policyName: xmlValue(xml, 'PolicyName'),
        arn: xmlValue(xml, 'Arn'),
        attachmentCount: xmlValue(xml, 'AttachmentCount'),
        defaultVersionId: xmlValue(xml, 'DefaultVersionId'),
        description: xmlValue(xml, 'Description') || undefined,
        createDate: xmlValue(xml, 'CreateDate') || undefined,
    };
}

export interface IamPolicyVersion {
    versionId: string;
    isDefaultVersion: boolean;
    createDate: string;
}

export async function iamListPolicyVersions(arn: string): Promise<{ versions: IamPolicyVersion[] }> {
    const xml = await iamRequest('ListPolicyVersions', { PolicyArn: arn });
    const raw = xmlArray(xml, 'member', ['VersionId', 'IsDefaultVersion', 'CreateDate']);
    return {
        versions: raw.map((v) => ({
            versionId: v['VersionId'] ?? '',
            isDefaultVersion: v['IsDefaultVersion'] === 'true',
            createDate: v['CreateDate'] ?? '',
        })),
    };
}

export async function iamGetPolicyVersion(arn: string, versionId: string): Promise<{ document: string; isDefaultVersion: boolean }> {
    const xml = await iamRequest('GetPolicyVersion', { PolicyArn: arn, VersionId: versionId });
    const doc = xmlValue(xml, 'Document');
    return {
        document: doc ? decodeURIComponent(doc) : '',
        isDefaultVersion: xmlValue(xml, 'IsDefaultVersion') === 'true',
    };
}

export async function iamCreatePolicyVersion(arn: string, document: string, setAsDefault = true): Promise<void> {
    await iamRequest('CreatePolicyVersion', {
        PolicyArn: arn,
        PolicyDocument: document,
        SetAsDefault: String(setAsDefault),
    });
}

export async function iamDeletePolicyVersion(arn: string, versionId: string): Promise<void> {
    await iamRequest('DeletePolicyVersion', { PolicyArn: arn, VersionId: versionId });
}

export async function iamCreatePolicy(name: string, document: string, description?: string): Promise<void> {
    const params: Record<string, string> = { PolicyName: name, PolicyDocument: document };
    if (description) params['Description'] = description;
    await iamRequest('CreatePolicy', params);
}

export async function iamDeletePolicy(arn: string): Promise<void> {
    await iamRequest('DeletePolicy', { PolicyArn: arn });
}

export async function iamCreateGroup(name: string): Promise<void> {
    await iamRequest('CreateGroup', { GroupName: name });
}

export async function iamDeleteGroup(name: string): Promise<void> {
    await iamRequest('DeleteGroup', { GroupName: name });
}

export async function iamGetGroup(name: string): Promise<{ group: IamGroup; users: IamUser[] }> {
    const xml = await iamRequest('GetGroup', { GroupName: name });
    const group: IamGroup = {
        groupName: xmlValue(xml, 'GroupName'),
        groupId: xmlValue(xml, 'GroupId'),
        arn: xmlValue(xml, 'Arn'),
    };
    const raw = xmlArray(xml, 'member', ['UserName', 'UserId', 'Arn', 'CreateDate']);
    const users: IamUser[] = raw
        .filter((u) => u['UserName'])
        .map((u) => ({
            userName: u['UserName'] ?? '',
            userId: u['UserId'] ?? '',
            arn: u['Arn'] ?? '',
            createDate: u['CreateDate'] ?? '',
        }));
    return { group, users };
}

export async function iamListAttachedGroupPolicies(groupName: string): Promise<{ policies: IamAttachedPolicy[] }> {
    const xml = await iamRequest('ListAttachedGroupPolicies', { GroupName: groupName });
    const raw = xmlArray(xml, 'member', ['PolicyName', 'PolicyArn']);
    return {
        policies: raw.map((p) => ({
            policyName: p['PolicyName'] ?? '',
            policyArn: p['PolicyArn'] ?? '',
        })),
    };
}

export async function iamAddUserToGroup(userName: string, groupName: string): Promise<void> {
    await iamRequest('AddUserToGroup', { UserName: userName, GroupName: groupName });
}

export async function iamRemoveUserFromGroup(userName: string, groupName: string): Promise<void> {
    await iamRequest('RemoveUserFromGroup', { UserName: userName, GroupName: groupName });
}

export async function iamAttachRolePolicy(roleName: string, policyArn: string): Promise<void> {
    await iamRequest('AttachRolePolicy', { RoleName: roleName, PolicyArn: policyArn });
}

export async function iamDetachRolePolicy(roleName: string, policyArn: string): Promise<void> {
    await iamRequest('DetachRolePolicy', { RoleName: roleName, PolicyArn: policyArn });
}

export async function iamAttachUserPolicy(userName: string, policyArn: string): Promise<void> {
    await iamRequest('AttachUserPolicy', { UserName: userName, PolicyArn: policyArn });
}

export async function iamDetachUserPolicy(userName: string, policyArn: string): Promise<void> {
    await iamRequest('DetachUserPolicy', { UserName: userName, PolicyArn: policyArn });
}

export async function iamAttachGroupPolicy(groupName: string, policyArn: string): Promise<void> {
    await iamRequest('AttachGroupPolicy', { GroupName: groupName, PolicyArn: policyArn });
}

export async function iamDetachGroupPolicy(groupName: string, policyArn: string): Promise<void> {
    await iamRequest('DetachGroupPolicy', { GroupName: groupName, PolicyArn: policyArn });
}

export interface IamAccountSummary {
    users: number;
    usersQuota: number;
    roles: number;
    rolesQuota: number;
    groups: number;
    groupsQuota: number;
    policies: number;
    policiesQuota: number;
    accessKeysPerUserQuota: number;
    accountAccessKeysPresent: number;
}

export async function iamGetAccountSummary(): Promise<IamAccountSummary> {
    const xml = await iamRequest('GetAccountSummary');
    function sumVal(key: string): number {
        const regex = new RegExp(`<key>${key}</key>\\s*<value>(\\d+)</value>`);
        const m = xml.match(regex);
        return m ? parseInt(m[1], 10) : 0;
    }
    return {
        users: sumVal('Users'),
        usersQuota: sumVal('UsersQuota'),
        roles: sumVal('Roles'),
        rolesQuota: sumVal('RolesQuota'),
        groups: sumVal('Groups'),
        groupsQuota: sumVal('GroupsQuota'),
        policies: sumVal('Policies'),
        policiesQuota: sumVal('PoliciesQuota'),
        accessKeysPerUserQuota: sumVal('AccessKeysPerUserQuota'),
        accountAccessKeysPresent: sumVal('AccountAccessKeysPresent'),
    };
}

export async function iamListAccountAliases(): Promise<{ aliases: string[] }> {
    const xml = await iamRequest('ListAccountAliases');
    const aliases: string[] = [];
    const regex = /<member>([^<]+)<\/member>/g;
    let m;
    while ((m = regex.exec(xml)) !== null) aliases.push(m[1]);
    return { aliases };
}

export async function iamCreateAccountAlias(alias: string): Promise<void> {
    await iamRequest('CreateAccountAlias', { AccountAlias: alias });
}

export async function iamDeleteAccountAlias(alias: string): Promise<void> {
    await iamRequest('DeleteAccountAlias', { AccountAlias: alias });
}

export interface IamPasswordPolicy {
    minimumPasswordLength: number;
    requireSymbols: boolean;
    requireNumbers: boolean;
    requireUppercaseCharacters: boolean;
    requireLowercaseCharacters: boolean;
    allowUsersToChangePassword: boolean;
    expirePasswords: boolean;
    maxPasswordAge: number;
    passwordReusePrevention: number;
    hardExpiry: boolean;
}

export async function iamGetAccountPasswordPolicy(): Promise<IamPasswordPolicy> {
    const xml = await iamRequest('GetAccountPasswordPolicy');
    return {
        minimumPasswordLength: parseInt(xmlValue(xml, 'MinimumPasswordLength') || '8', 10),
        requireSymbols: xmlValue(xml, 'RequireSymbols') === 'true',
        requireNumbers: xmlValue(xml, 'RequireNumbers') === 'true',
        requireUppercaseCharacters: xmlValue(xml, 'RequireUppercaseCharacters') === 'true',
        requireLowercaseCharacters: xmlValue(xml, 'RequireLowercaseCharacters') === 'true',
        allowUsersToChangePassword: xmlValue(xml, 'AllowUsersToChangePassword') === 'true',
        expirePasswords: xmlValue(xml, 'ExpirePasswords') === 'true',
        maxPasswordAge: parseInt(xmlValue(xml, 'MaxPasswordAge') || '0', 10),
        passwordReusePrevention: parseInt(xmlValue(xml, 'PasswordReusePrevention') || '0', 10),
        hardExpiry: xmlValue(xml, 'HardExpiry') === 'true',
    };
}

export async function iamUpdateAccountPasswordPolicy(policy: Partial<IamPasswordPolicy>): Promise<void> {
    const params: Record<string, string> = {};
    if (policy.minimumPasswordLength !== undefined) params['MinimumPasswordLength'] = String(policy.minimumPasswordLength);
    if (policy.requireSymbols !== undefined) params['RequireSymbols'] = String(policy.requireSymbols);
    if (policy.requireNumbers !== undefined) params['RequireNumbers'] = String(policy.requireNumbers);
    if (policy.requireUppercaseCharacters !== undefined) params['RequireUppercaseCharacters'] = String(policy.requireUppercaseCharacters);
    if (policy.requireLowercaseCharacters !== undefined) params['RequireLowercaseCharacters'] = String(policy.requireLowercaseCharacters);
    if (policy.allowUsersToChangePassword !== undefined) params['AllowUsersToChangePassword'] = String(policy.allowUsersToChangePassword);
    if (policy.maxPasswordAge !== undefined) params['MaxPasswordAge'] = String(policy.maxPasswordAge);
    if (policy.passwordReusePrevention !== undefined) params['PasswordReusePrevention'] = String(policy.passwordReusePrevention);
    await iamRequest('UpdateAccountPasswordPolicy', params);
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
    const res = await loggedFetch('lambda', 'ListFunctions', 'GET', `${ENDPOINT}/2015-03-31/functions`, {
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
    const res = await loggedFetch('lambda', 'CreateFunction', 'POST', `${ENDPOINT}/2015-03-31/functions`, {
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
    const res = await loggedFetch('lambda', 'DeleteFunction', 'DELETE', `${ENDPOINT}/2015-03-31/functions/${encodeURIComponent(name)}`, {
        method: 'DELETE',
        headers: lambdaHeaders(),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}

export async function invokeFunction(name: string, payload: string): Promise<string> {
    const res = await loggedFetch('lambda', 'Invoke', 'POST', `${ENDPOINT}/2015-03-31/functions/${encodeURIComponent(name)}/invocations`, {
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
    const res = await loggedFetch('cognito-idp', action, 'POST', ENDPOINT, {
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

export async function createUserPool(
    poolName: string,
    options?: { mfaConfig?: string },
): Promise<void> {
    const body: Record<string, unknown> = { PoolName: poolName };
    if (options?.mfaConfig) body.MfaConfiguration = options.mfaConfig;
    await cognitoRequest('CreateUserPool', body);
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

export async function describeUserPool(userPoolId: string): Promise<unknown> {
    return cognitoRequest('DescribeUserPool', { UserPoolId: userPoolId });
}

export async function updateUserPool(userPoolId: string, config: unknown): Promise<void> {
    await cognitoRequest('UpdateUserPool', { UserPoolId: userPoolId, ...(config as Record<string, unknown>) });
}

// Domain

export async function createUserPoolDomain(userPoolId: string, domain: string): Promise<void> {
    await cognitoRequest('CreateUserPoolDomain', { UserPoolId: userPoolId, Domain: domain });
}

export async function deleteUserPoolDomain(userPoolId: string, domain: string): Promise<void> {
    await cognitoRequest('DeleteUserPoolDomain', { UserPoolId: userPoolId, Domain: domain });
}

// Resource Servers

export interface CognitoResourceServerScope {
    name: string;
    description: string;
}

export interface CognitoResourceServer {
    identifier: string;
    name: string;
    scopes: CognitoResourceServerScope[];
}

export async function listResourceServers(userPoolId: string): Promise<{ servers: CognitoResourceServer[] }> {
    const data = await cognitoRequest('ListResourceServers', { UserPoolId: userPoolId, MaxResults: 50 }) as {
        ResourceServers?: { Identifier: string; Name: string; Scopes?: { ScopeName: string; ScopeDescription: string }[] }[]
    };
    return {
        servers: (data.ResourceServers ?? []).map((s) => ({
            identifier: s.Identifier,
            name: s.Name,
            scopes: (s.Scopes ?? []).map((sc) => ({ name: sc.ScopeName, description: sc.ScopeDescription })),
        })),
    };
}

export async function createResourceServer(
    userPoolId: string,
    identifier: string,
    name: string,
    scopes: CognitoResourceServerScope[]
): Promise<void> {
    await cognitoRequest('CreateResourceServer', {
        UserPoolId: userPoolId,
        Identifier: identifier,
        Name: name,
        Scopes: scopes.map((s) => ({ ScopeName: s.name, ScopeDescription: s.description })),
    });
}

export async function deleteResourceServer(userPoolId: string, identifier: string): Promise<void> {
    await cognitoRequest('DeleteResourceServer', { UserPoolId: userPoolId, Identifier: identifier });
}

// Identity Providers (User Pool)

export interface CognitoIdentityProvider {
    providerName: string;
    providerType: string;
    creationDate?: string;
}

export async function listIdentityProviders(userPoolId: string): Promise<{ providers: CognitoIdentityProvider[] }> {
    const data = await cognitoRequest('ListIdentityProviders', { UserPoolId: userPoolId, MaxResults: 60 }) as {
        Providers?: { ProviderName: string; ProviderType: string; CreationDate?: number }[]
    };
    return {
        providers: (data.Providers ?? []).map((p) => ({
            providerName: p.ProviderName,
            providerType: p.ProviderType,
            creationDate: p.CreationDate ? new Date(p.CreationDate * 1000).toISOString() : undefined,
        })),
    };
}

export async function createIdentityProvider(
    userPoolId: string,
    providerName: string,
    providerType: string,
    providerDetails: Record<string, string>,
    attributeMapping: Record<string, string>
): Promise<void> {
    await cognitoRequest('CreateIdentityProvider', {
        UserPoolId: userPoolId,
        ProviderName: providerName,
        ProviderType: providerType,
        ProviderDetails: providerDetails,
        AttributeMapping: attributeMapping,
    });
}

export async function deleteIdentityProvider(userPoolId: string, providerName: string): Promise<void> {
    await cognitoRequest('DeleteIdentityProvider', { UserPoolId: userPoolId, ProviderName: providerName });
}

// Custom Attributes

export async function addCustomAttributes(
    userPoolId: string,
    attrs: { Name: string; AttributeDataType: string }[]
): Promise<void> {
    await cognitoRequest('AddCustomAttributes', {
        UserPoolId: userPoolId,
        CustomAttributes: attrs.map((a) => ({ Name: a.Name, AttributeDataType: a.AttributeDataType })),
    });
}

// User Pool Clients

export interface CognitoUserPoolClient {
    clientId: string;
    clientName: string;
    userPoolId: string;
}

export interface CognitoUserPoolClientDetail {
    clientId: string;
    clientName: string;
    clientSecret?: string;
    explicitAuthFlows: string[];
    callbackUrLs: string[];
    allowedOAuthScopes: string[];
}

export async function listUserPoolClients(userPoolId: string): Promise<{ clients: CognitoUserPoolClient[] }> {
    const data = await cognitoRequest('ListUserPoolClients', { UserPoolId: userPoolId, MaxResults: 60 }) as {
        UserPoolClients?: { ClientId: string; ClientName: string; UserPoolId: string }[]
    };
    return {
        clients: (data.UserPoolClients ?? []).map((c) => ({
            clientId: c.ClientId,
            clientName: c.ClientName,
            userPoolId: c.UserPoolId,
        })),
    };
}

export async function createUserPoolClient(
    userPoolId: string,
    clientName: string,
    opts?: { generateSecret?: boolean; authFlows?: string[]; callbackUrls?: string[] }
): Promise<{ clientId: string }> {
    const body: Record<string, unknown> = { UserPoolId: userPoolId, ClientName: clientName };
    if (opts?.generateSecret !== undefined) body['GenerateSecret'] = opts.generateSecret;
    if (opts?.authFlows) body['ExplicitAuthFlows'] = opts.authFlows;
    if (opts?.callbackUrls) body['CallbackURLs'] = opts.callbackUrls;
    const data = await cognitoRequest('CreateUserPoolClient', body) as {
        UserPoolClient?: { ClientId: string }
    };
    return { clientId: data.UserPoolClient?.ClientId ?? '' };
}

export async function describeUserPoolClient(userPoolId: string, clientId: string): Promise<CognitoUserPoolClientDetail> {
    const data = await cognitoRequest('DescribeUserPoolClient', { UserPoolId: userPoolId, ClientId: clientId }) as {
        UserPoolClient?: {
            ClientId: string; ClientName: string; ClientSecret?: string;
            ExplicitAuthFlows?: string[]; CallbackURLs?: string[]; AllowedOAuthScopes?: string[]
        }
    };
    const c = data.UserPoolClient ?? {} as NonNullable<typeof data.UserPoolClient>;
    return {
        clientId: c?.ClientId ?? clientId,
        clientName: c?.ClientName ?? '',
        clientSecret: c?.ClientSecret,
        explicitAuthFlows: c?.ExplicitAuthFlows ?? [],
        callbackUrLs: c?.CallbackURLs ?? [],
        allowedOAuthScopes: c?.AllowedOAuthScopes ?? [],
    };
}

export async function deleteUserPoolClient(userPoolId: string, clientId: string): Promise<void> {
    await cognitoRequest('DeleteUserPoolClient', { UserPoolId: userPoolId, ClientId: clientId });
}

// Users

export interface CognitoUserDetail {
    username: string;
    status: string;
    enabled: boolean;
    createDate: string;
    attributes: { name: string; value: string }[];
}

export async function adminCreateUser(
    userPoolId: string,
    username: string,
    opts?: { tempPassword?: string; email?: string }
): Promise<void> {
    const body: Record<string, unknown> = { UserPoolId: userPoolId, Username: username };
    if (opts?.tempPassword) body['TemporaryPassword'] = opts.tempPassword;
    if (opts?.email) body['UserAttributes'] = [{ Name: 'email', Value: opts.email }];
    await cognitoRequest('AdminCreateUser', body);
}

export async function adminDeleteUser(userPoolId: string, username: string): Promise<void> {
    await cognitoRequest('AdminDeleteUser', { UserPoolId: userPoolId, Username: username });
}

export async function adminGetUser(userPoolId: string, username: string): Promise<CognitoUserDetail> {
    const data = await cognitoRequest('AdminGetUser', { UserPoolId: userPoolId, Username: username }) as {
        Username?: string; UserStatus?: string; Enabled?: boolean;
        UserCreateDate?: number;
        UserAttributes?: { Name: string; Value: string }[]
    };
    return {
        username: data.Username ?? username,
        status: data.UserStatus ?? '',
        enabled: data.Enabled ?? true,
        createDate: data.UserCreateDate ? new Date(data.UserCreateDate * 1000).toISOString() : '',
        attributes: (data.UserAttributes ?? []).map((a) => ({ name: a.Name, value: a.Value })),
    };
}

export async function adminSetUserPassword(
    userPoolId: string,
    username: string,
    password: string,
    permanent = true
): Promise<void> {
    await cognitoRequest('AdminSetUserPassword', { UserPoolId: userPoolId, Username: username, Password: password, Permanent: permanent });
}

export async function adminEnableUser(userPoolId: string, username: string): Promise<void> {
    await cognitoRequest('AdminEnableUser', { UserPoolId: userPoolId, Username: username });
}

export async function adminDisableUser(userPoolId: string, username: string): Promise<void> {
    await cognitoRequest('AdminDisableUser', { UserPoolId: userPoolId, Username: username });
}

export async function adminUpdateUserAttributes(
    userPoolId: string,
    username: string,
    attrs: { Name: string; Value: string }[]
): Promise<void> {
    await cognitoRequest('AdminUpdateUserAttributes', { UserPoolId: userPoolId, Username: username, UserAttributes: attrs });
}

export async function listCognitoUsersWithAttrs(userPoolId: string): Promise<{ users: CognitoUser[] }> {
    return listCognitoUsers(userPoolId);
}

// Groups

export interface CognitoGroup {
    name: string;
    description: string;
    roleArn: string;
    precedence?: number;
}

export async function listCognitoGroups(userPoolId: string): Promise<{ groups: CognitoGroup[] }> {
    const data = await cognitoRequest('ListGroups', { UserPoolId: userPoolId }) as {
        Groups?: { GroupName: string; Description?: string; RoleArn?: string; Precedence?: number }[]
    };
    return {
        groups: (data.Groups ?? []).map((g) => ({
            name: g.GroupName,
            description: g.Description ?? '',
            roleArn: g.RoleArn ?? '',
            precedence: g.Precedence,
        })),
    };
}

export async function createCognitoGroup(
    userPoolId: string,
    name: string,
    opts?: { description?: string; roleArn?: string; precedence?: number }
): Promise<void> {
    const body: Record<string, unknown> = { UserPoolId: userPoolId, GroupName: name };
    if (opts?.description) body['Description'] = opts.description;
    if (opts?.roleArn) body['RoleArn'] = opts.roleArn;
    if (opts?.precedence != null) body['Precedence'] = opts.precedence;
    await cognitoRequest('CreateGroup', body);
}

export async function deleteCognitoGroup(userPoolId: string, name: string): Promise<void> {
    await cognitoRequest('DeleteGroup', { UserPoolId: userPoolId, GroupName: name });
}

export async function adminAddUserToGroup(userPoolId: string, username: string, groupName: string): Promise<void> {
    await cognitoRequest('AdminAddUserToGroup', { UserPoolId: userPoolId, Username: username, GroupName: groupName });
}

export async function adminRemoveUserFromGroup(userPoolId: string, username: string, groupName: string): Promise<void> {
    await cognitoRequest('AdminRemoveUserFromGroup', { UserPoolId: userPoolId, Username: username, GroupName: groupName });
}

export async function listUsersInGroup(userPoolId: string, groupName: string): Promise<{ users: CognitoUser[] }> {
    const data = await cognitoRequest('ListUsersInGroup', { UserPoolId: userPoolId, GroupName: groupName }) as {
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

export async function adminListGroupsForUser(userPoolId: string, username: string): Promise<{ groups: CognitoGroup[] }> {
    const data = await cognitoRequest('AdminListGroupsForUser', { UserPoolId: userPoolId, Username: username }) as {
        Groups?: { GroupName: string; Description?: string; RoleArn?: string; Precedence?: number }[]
    };
    return {
        groups: (data.Groups ?? []).map((g) => ({
            name: g.GroupName,
            description: g.Description ?? '',
            roleArn: g.RoleArn ?? '',
            precedence: g.Precedence,
        })),
    };
}

// Auth Testing

export async function cognitoSignUp(
    clientId: string,
    username: string,
    password: string,
    email?: string
): Promise<unknown> {
    const body: Record<string, unknown> = { ClientId: clientId, Username: username, Password: password };
    if (email) body['UserAttributes'] = [{ Name: 'email', Value: email }];
    return cognitoRequest('SignUp', body);
}

export async function cognitoInitiateAuth(
    clientId: string,
    username: string,
    password: string
): Promise<{
    accessToken?: string; idToken?: string; refreshToken?: string;
    expiresIn?: number; tokenType?: string; challengeName?: string
}> {
    const data = await cognitoRequest('InitiateAuth', {
        ClientId: clientId,
        AuthFlow: 'USER_PASSWORD_AUTH',
        AuthParameters: { USERNAME: username, PASSWORD: password },
    }) as {
        AuthenticationResult?: {
            AccessToken?: string; IdToken?: string; RefreshToken?: string;
            ExpiresIn?: number; TokenType?: string
        };
        ChallengeName?: string;
    };
    return {
        accessToken: data.AuthenticationResult?.AccessToken,
        idToken: data.AuthenticationResult?.IdToken,
        refreshToken: data.AuthenticationResult?.RefreshToken,
        expiresIn: data.AuthenticationResult?.ExpiresIn,
        tokenType: data.AuthenticationResult?.TokenType,
        challengeName: data.ChallengeName,
    };
}

export async function cognitoGetUser(accessToken: string): Promise<unknown> {
    return cognitoRequest('GetUser', { AccessToken: accessToken });
}

// Identity Pools

async function cognitoIdentityRequest(action: string, body: unknown): Promise<unknown> {
    const res = await loggedFetch('cognito-identity', action, 'POST', ENDPOINT, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/x-amz-json-1.1',
            'X-Amz-Target': `AWSCognitoIdentityService.${action}`,
            'Authorization': authHeader('cognito-identity'),
            'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
        },
        body: JSON.stringify(body),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
    return res.json();
}

export interface CognitoIdentityPool {
    id: string;
    name: string;
    allowUnauthenticated: boolean;
}

export async function listIdentityPools(): Promise<{ identityPools: CognitoIdentityPool[] }> {
    const data = await cognitoIdentityRequest('ListIdentityPools', { MaxResults: 60 }) as {
        IdentityPools?: { IdentityPoolId: string; IdentityPoolName: string; AllowUnauthenticatedIdentities?: boolean }[]
    };
    return {
        identityPools: (data.IdentityPools ?? []).map((p) => ({
            id: p.IdentityPoolId,
            name: p.IdentityPoolName,
            allowUnauthenticated: p.AllowUnauthenticatedIdentities ?? false,
        })),
    };
}

export async function createIdentityPool(name: string, allowUnauth: boolean): Promise<{ id: string }> {
    const data = await cognitoIdentityRequest('CreateIdentityPool', {
        IdentityPoolName: name,
        AllowUnauthenticatedIdentities: allowUnauth,
    }) as { IdentityPoolId?: string };
    return { id: data.IdentityPoolId ?? '' };
}

export async function deleteIdentityPool(id: string): Promise<void> {
    await cognitoIdentityRequest('DeleteIdentityPool', { IdentityPoolId: id });
}

export async function describeIdentityPool(id: string): Promise<unknown> {
    return cognitoIdentityRequest('DescribeIdentityPool', { IdentityPoolId: id });
}

export async function setIdentityPoolRoles(poolId: string, roles: Record<string, string>): Promise<void> {
    await cognitoIdentityRequest('SetIdentityPoolRoles', {
        IdentityPoolId: poolId,
        Roles: roles,
    });
}

export async function getIdentityPoolRoles(poolId: string): Promise<unknown> {
    return cognitoIdentityRequest('GetIdentityPoolRoles', {
        IdentityPoolId: poolId,
    });
}

export async function cognitoGetId(identityPoolId: string, logins?: Record<string, string>): Promise<unknown> {
    return cognitoIdentityRequest('GetId', {
        IdentityPoolId: identityPoolId,
        ...(logins ? { Logins: logins } : {}),
    });
}

export async function cognitoGetCredentials(identityId: string, logins?: Record<string, string>): Promise<unknown> {
    return cognitoIdentityRequest('GetCredentialsForIdentity', {
        IdentityId: identityId,
        ...(logins ? { Logins: logins } : {}),
    });
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
    const res = await loggedFetch('ses', 'ListEmailIdentities', 'GET', `${ENDPOINT}/v2/email/identities`, {
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
    const res = await loggedFetch('ses', 'CreateEmailIdentity', 'POST', `${ENDPOINT}/v2/email/identities`, {
        method: 'POST',
        headers: sesHeaders(),
        body: JSON.stringify({ EmailIdentity: emailIdentity }),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}

export async function deleteEmailIdentity(emailIdentity: string): Promise<void> {
    const res = await loggedFetch('ses', 'DeleteEmailIdentity', 'DELETE', `${ENDPOINT}/v2/email/identities/${encodeURIComponent(emailIdentity)}`, {
        method: 'DELETE',
        headers: sesHeaders(),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}

export async function listEmailTemplates(): Promise<{ templates: SesTemplate[] }> {
    const res = await loggedFetch('ses', 'ListEmailTemplates', 'GET', `${ENDPOINT}/v2/email/templates`, {
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
    const res = await loggedFetch('ses', 'CreateEmailTemplate', 'POST', `${ENDPOINT}/v2/email/templates`, {
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
    const res = await loggedFetch('ses', 'DeleteEmailTemplate', 'DELETE', `${ENDPOINT}/v2/email/templates/${encodeURIComponent(templateName)}`, {
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

export async function describeStateMachine(arn: string): Promise<{ definition: string; name: string; type: string; status: string }> {
    const data = await awsRequest('states', 'DescribeStateMachine', { stateMachineArn: arn } as unknown as Record<string, string>, 'json', 'AWSStepFunctions') as {
        definition?: string; name?: string; type?: string; status?: string;
    };
    return {
        definition: data.definition ?? '{}',
        name: data.name ?? '',
        type: data.type ?? '',
        status: data.status ?? '',
    };
}

export interface SfnHistoryEvent {
    id: number;
    type: string;
    timestamp: number;
    stateEnteredEventDetails?: { name: string; input?: string };
    stateExitedEventDetails?: { name: string; output?: string };
    taskSucceededEventDetails?: { output?: string };
    taskFailedEventDetails?: { error?: string; cause?: string };
}

export async function getExecutionHistory(arn: string): Promise<{ events: SfnHistoryEvent[] }> {
    const data = await awsRequest('states', 'GetExecutionHistory', { executionArn: arn } as unknown as Record<string, string>, 'json', 'AWSStepFunctions') as {
        events?: {
            id: number; type: string; timestamp: number;
            stateEnteredEventDetails?: { name: string; input?: string };
            stateExitedEventDetails?: { name: string; output?: string };
            taskSucceededEventDetails?: { output?: string };
            taskFailedEventDetails?: { error?: string; cause?: string };
        }[]
    };
    return { events: data.events ?? [] };
}

export async function describeExecution(arn: string): Promise<{ status: string; input?: string; output?: string; startDate: number; stopDate?: number; name: string }> {
    const data = await awsRequest('states', 'DescribeExecution', { executionArn: arn } as unknown as Record<string, string>, 'json', 'AWSStepFunctions') as {
        status?: string; input?: string; output?: string; startDate?: number; stopDate?: number; name?: string;
    };
    return {
        status: data.status ?? '',
        input: data.input,
        output: data.output,
        startDate: data.startDate ?? 0,
        stopDate: data.stopDate,
        name: data.name ?? '',
    };
}

// ---- EC2 ----

async function ec2Request(action: string, params: Record<string, string> = {}): Promise<string> {
    const body = new URLSearchParams({ Action: action, Version: '2016-11-15', ...params });
    const res = await loggedFetch('ec2', action, 'POST', ENDPOINT, {
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
    const res = await loggedFetch('cloudformation', action, 'POST', ENDPOINT, {
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

// ---- STS ----

export interface StsIdentity {
    account: string;
    arn: string;
    userId: string;
}

export interface StsCredentials {
    accessKeyId: string;
    secretAccessKey: string;
    sessionToken: string;
    expiration: string;
}

async function stsRequest(action: string, params: Record<string, string> = {}): Promise<string> {
    const body = new URLSearchParams({ Action: action, Version: '2011-06-15', ...params });
    const res = await loggedFetch('sts', action, 'POST', ENDPOINT, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/x-www-form-urlencoded',
            'Authorization': authHeader('sts'),
            'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
        },
        body: body.toString(),
    });
    return res.text();
}

export async function getCallerIdentity(): Promise<StsIdentity> {
    const xml = await stsRequest('GetCallerIdentity');
    return {
        account: xmlValue(xml, 'Account'),
        arn: xmlValue(xml, 'Arn'),
        userId: xmlValue(xml, 'UserId'),
    };
}

export async function assumeRole(roleArn: string, roleSessionName: string): Promise<StsCredentials> {
    const xml = await stsRequest('AssumeRole', { RoleArn: roleArn, RoleSessionName: roleSessionName });
    return {
        accessKeyId: xmlValue(xml, 'AccessKeyId'),
        secretAccessKey: xmlValue(xml, 'SecretAccessKey'),
        sessionToken: xmlValue(xml, 'SessionToken'),
        expiration: xmlValue(xml, 'Expiration'),
    };
}

// ---- RDS ----

async function rdsRequest(action: string, params: Record<string, string> = {}): Promise<string> {
    const body = new URLSearchParams({ Action: action, Version: '2014-10-31', ...params });
    const res = await loggedFetch('rds', action, 'POST', ENDPOINT, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/x-www-form-urlencoded',
            'Authorization': authHeader('rds'),
            'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
        },
        body: body.toString(),
    });
    return res.text();
}

export async function listDbInstances(): Promise<string> {
    return rdsRequest('DescribeDBInstances');
}

export async function createDbInstance(id: string, engine: string, instanceClass: string): Promise<string> {
    return rdsRequest('CreateDBInstance', {
        DBInstanceIdentifier: id,
        Engine: engine,
        DBInstanceClass: instanceClass,
        MasterUsername: 'admin',
        MasterUserPassword: 'password123',
        AllocatedStorage: '20',
    });
}

export async function deleteDbInstance(id: string): Promise<string> {
    return rdsRequest('DeleteDBInstance', { DBInstanceIdentifier: id, SkipFinalSnapshot: 'true' });
}

export async function listDbClusters(): Promise<string> {
    return rdsRequest('DescribeDBClusters');
}

// ---- ELB (Elastic Load Balancing v2) ----

async function elbRequest(action: string, params: Record<string, string> = {}): Promise<string> {
    const body = new URLSearchParams({ Action: action, Version: '2015-12-01', ...params });
    const res = await loggedFetch('elasticloadbalancing', action, 'POST', ENDPOINT, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/x-www-form-urlencoded',
            'Authorization': authHeader('elasticloadbalancing'),
            'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
        },
        body: body.toString(),
    });
    return res.text();
}

export interface ElbLoadBalancer {
    arn: string;
    name: string;
    dnsName: string;
    type: string;
    scheme: string;
    state: string;
    createdTime: string;
}

export interface ElbTargetGroup {
    arn: string;
    name: string;
    protocol: string;
    port: string;
    vpcId: string;
    targetType: string;
}

export interface ElbListener {
    arn: string;
    loadBalancerArn: string;
    port: string;
    protocol: string;
}

export async function describeLoadBalancers(): Promise<{ loadBalancers: ElbLoadBalancer[] }> {
    const xml = await elbRequest('DescribeLoadBalancers');
    const raw = xmlArray(xml, 'member', ['LoadBalancerArn', 'LoadBalancerName', 'DNSName', 'Type', 'Scheme', 'CreatedTime']);
    return {
        loadBalancers: raw.map((lb) => ({
            arn: lb['LoadBalancerArn'] ?? '',
            name: lb['LoadBalancerName'] ?? '',
            dnsName: lb['DNSName'] ?? '',
            type: lb['Type'] ?? '',
            scheme: lb['Scheme'] ?? '',
            state: 'active',
            createdTime: lb['CreatedTime'] ?? '',
        })),
    };
}

export async function createLoadBalancer(
    name: string,
    type: string,
    scheme: string,
): Promise<void> {
    await elbRequest('CreateLoadBalancer', { Name: name, Type: type, Scheme: scheme });
}

export async function deleteLoadBalancer(arn: string): Promise<void> {
    await elbRequest('DeleteLoadBalancer', { LoadBalancerArn: arn });
}

export async function describeTargetGroups(): Promise<{ targetGroups: ElbTargetGroup[] }> {
    const xml = await elbRequest('DescribeTargetGroups');
    const raw = xmlArray(xml, 'member', ['TargetGroupArn', 'TargetGroupName', 'Protocol', 'Port', 'VpcId', 'TargetType']);
    return {
        targetGroups: raw.map((tg) => ({
            arn: tg['TargetGroupArn'] ?? '',
            name: tg['TargetGroupName'] ?? '',
            protocol: tg['Protocol'] ?? '',
            port: tg['Port'] ?? '',
            vpcId: tg['VpcId'] ?? '',
            targetType: tg['TargetType'] ?? '',
        })),
    };
}

export async function createTargetGroup(
    name: string,
    protocol: string,
    port: number,
    targetType: string,
): Promise<void> {
    await elbRequest('CreateTargetGroup', {
        Name: name,
        Protocol: protocol,
        Port: String(port),
        TargetType: targetType,
    });
}

export async function deleteTargetGroup(arn: string): Promise<void> {
    await elbRequest('DeleteTargetGroup', { TargetGroupArn: arn });
}

export async function describeListeners(loadBalancerArn: string): Promise<{ listeners: ElbListener[] }> {
    const xml = await elbRequest('DescribeListeners', { LoadBalancerArn: loadBalancerArn });
    const raw = xmlArray(xml, 'member', ['ListenerArn', 'LoadBalancerArn', 'Port', 'Protocol']);
    return {
        listeners: raw.map((l) => ({
            arn: l['ListenerArn'] ?? '',
            loadBalancerArn: l['LoadBalancerArn'] ?? '',
            port: l['Port'] ?? '',
            protocol: l['Protocol'] ?? '',
        })),
    };
}

// ---- CloudFront ----

function cloudfrontHeaders(): Record<string, string> {
    return {
        'Authorization': authHeader('cloudfront'),
        'X-Amz-Date': new Date().toISOString().replace(/[:-]/g, '').slice(0, 15) + 'Z',
        'Content-Type': 'application/xml',
    };
}

export interface CloudFrontDistribution {
    id: string;
    arn: string;
    domainName: string;
    status: string;
    comment: string;
    enabled: boolean;
    createdAt: string;
}

export async function listDistributions(): Promise<{ distributions: CloudFrontDistribution[] }> {
    const res = await loggedFetch('cloudfront', 'ListDistributions', 'GET', `${ENDPOINT}/2020-05-31/distribution`, {
        headers: cloudfrontHeaders(),
    });
    const text = await res.text();

    const distributions: CloudFrontDistribution[] = [];
    const itemRegex = /<DistributionSummary>([\s\S]*?)<\/DistributionSummary>/g;
    let match;
    while ((match = itemRegex.exec(text)) !== null) {
        const block = match[1];
        distributions.push({
            id: xmlValue(block, 'Id'),
            arn: xmlValue(block, 'ARN'),
            domainName: xmlValue(block, 'DomainName'),
            status: xmlValue(block, 'Status'),
            comment: xmlValue(block, 'Comment'),
            enabled: xmlValue(block, 'Enabled') === 'true',
            createdAt: xmlValue(block, 'LastModifiedTime'),
        });
    }
    return { distributions };
}

export async function createDistribution(originDomain: string, comment: string): Promise<void> {
    const body = `<?xml version="1.0" encoding="UTF-8"?>
<DistributionConfig>
  <CallerReference>${Date.now()}</CallerReference>
  <Origins>
    <Quantity>1</Quantity>
    <Items>
      <Origin>
        <Id>origin-1</Id>
        <DomainName>${originDomain}</DomainName>
      </Origin>
    </Items>
  </Origins>
  <DefaultCacheBehavior>
    <ViewerProtocolPolicy>redirect-to-https</ViewerProtocolPolicy>
    <TargetOriginId>origin-1</TargetOriginId>
    <ForwardedValues>
      <QueryString>false</QueryString>
      <Cookies><Forward>none</Forward></Cookies>
    </ForwardedValues>
    <MinTTL>0</MinTTL>
  </DefaultCacheBehavior>
  <Comment>${comment}</Comment>
  <Enabled>true</Enabled>
</DistributionConfig>`;

    const res = await loggedFetch('cloudfront', 'CreateDistribution', 'POST', `${ENDPOINT}/2020-05-31/distribution`, {
        method: 'POST',
        headers: cloudfrontHeaders(),
        body,
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}

export async function deleteDistribution(id: string): Promise<void> {
    const res = await loggedFetch('cloudfront', 'DeleteDistribution', 'DELETE', `${ENDPOINT}/2020-05-31/distribution/${id}`, {
        method: 'DELETE',
        headers: cloudfrontHeaders(),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
}
