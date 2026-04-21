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

// ---- SQS ----

export interface SqsQueue {
    url: string;
    name: string;
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

// ---- DynamoDB ----

export interface DynamoTable {
    name: string;
}

export async function listTables(): Promise<{ tables: DynamoTable[] }> {
    const data = await awsRequest('dynamodb', 'ListTables', {}, 'json', 'DynamoDB_20120810') as { TableNames?: string[] };
    return {
        tables: (data.TableNames ?? []).map((name) => ({ name })),
    };
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
