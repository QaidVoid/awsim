/**
 * Typed Secrets Manager API client.
 *
 * Uses the secretsmanager JSON 1.1 protocol.
 */

const ENDPOINT = "http://localhost:4566";
const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, "");

function authHeader(): string {
  return `AWS4-HMAC-SHA256 Credential=awsim-admin/${FAKE_DATE}/us-east-1/secretsmanager/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

function amzDate(): string {
  return new Date().toISOString().replace(/[:-]/g, "").slice(0, 15) + "Z";
}

async function smRequest(action: string, body: unknown = {}): Promise<unknown> {
  const res = await fetch(ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `secretsmanager.${action}`,
      Authorization: authHeader(),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok)
    throw new Error(`SecretsManager ${action} failed: ${res.status} ${text}`);
  return text ? JSON.parse(text) : {};
}

// ---- Types ----

export interface Secret {
  name: string;
  arn: string;
  description?: string;
  lastChangedDate?: string;
  lastAccessedDate?: string;
  rotationEnabled?: boolean;
}

export interface SecretDetail extends Secret {
  kmsKeyId?: string;
  rotationLambdaArn?: string;
  rotationRules?: { automaticallyAfterDays?: number };
  versionIdsToStages?: Record<string, string[]>;
  createdDate?: string;
}

export interface SecretVersion {
  versionId: string;
  stages: string[];
  createdDate?: string;
  lastAccessedDate?: string;
}

export interface SecretValue {
  arn: string;
  name: string;
  versionId?: string;
  secretString?: string;
  secretBinary?: string;
  versionStages?: string[];
  createdDate?: string;
}

// ---- Operations ----

export async function listSecrets(): Promise<Secret[]> {
  const data = (await smRequest("ListSecrets")) as {
    SecretList?: {
      Name: string;
      ARN: string;
      Description?: string;
      LastChangedDate?: number;
      LastAccessedDate?: number;
      RotationEnabled?: boolean;
    }[];
  };
  return (data.SecretList ?? []).map((s) => ({
    name: s.Name,
    arn: s.ARN,
    description: s.Description,
    lastChangedDate: s.LastChangedDate
      ? new Date(s.LastChangedDate * 1000).toISOString()
      : undefined,
    lastAccessedDate: s.LastAccessedDate
      ? new Date(s.LastAccessedDate * 1000).toISOString()
      : undefined,
    rotationEnabled: s.RotationEnabled,
  }));
}

export async function describeSecret(secretId: string): Promise<SecretDetail> {
  const data = (await smRequest("DescribeSecret", { SecretId: secretId })) as {
    Name?: string;
    ARN?: string;
    Description?: string;
    KmsKeyId?: string;
    RotationEnabled?: boolean;
    RotationLambdaARN?: string;
    RotationRules?: { AutomaticallyAfterDays?: number };
    LastChangedDate?: number;
    LastAccessedDate?: number;
    CreatedDate?: number;
    VersionIdsToStages?: Record<string, string[]>;
  };
  return {
    name: data.Name ?? secretId,
    arn: data.ARN ?? "",
    description: data.Description,
    kmsKeyId: data.KmsKeyId,
    rotationEnabled: data.RotationEnabled ?? false,
    rotationLambdaArn: data.RotationLambdaARN,
    rotationRules: data.RotationRules
      ? { automaticallyAfterDays: data.RotationRules.AutomaticallyAfterDays }
      : undefined,
    lastChangedDate: data.LastChangedDate
      ? new Date(data.LastChangedDate * 1000).toISOString()
      : undefined,
    lastAccessedDate: data.LastAccessedDate
      ? new Date(data.LastAccessedDate * 1000).toISOString()
      : undefined,
    createdDate: data.CreatedDate
      ? new Date(data.CreatedDate * 1000).toISOString()
      : undefined,
    versionIdsToStages: data.VersionIdsToStages,
  };
}

export async function getSecretValue(
  secretId: string,
  versionId?: string,
): Promise<SecretValue> {
  const body: Record<string, string> = { SecretId: secretId };
  if (versionId) body.VersionId = versionId;
  const data = (await smRequest("GetSecretValue", body)) as {
    ARN?: string;
    Name?: string;
    VersionId?: string;
    SecretString?: string;
    SecretBinary?: string;
    VersionStages?: string[];
    CreatedDate?: number;
  };
  return {
    arn: data.ARN ?? "",
    name: data.Name ?? secretId,
    versionId: data.VersionId,
    secretString: data.SecretString,
    secretBinary: data.SecretBinary,
    versionStages: data.VersionStages,
    createdDate: data.CreatedDate
      ? new Date(data.CreatedDate * 1000).toISOString()
      : undefined,
  };
}

export async function listSecretVersions(
  secretId: string,
): Promise<SecretVersion[]> {
  const data = (await smRequest("ListSecretVersionIds", {
    SecretId: secretId,
    IncludeDeprecated: true,
  })) as {
    Versions?: {
      VersionId: string;
      VersionStages?: string[];
      CreatedDate?: number;
      LastAccessedDate?: number;
    }[];
  };
  return (data.Versions ?? []).map((v) => ({
    versionId: v.VersionId,
    stages: v.VersionStages ?? [],
    createdDate: v.CreatedDate
      ? new Date(v.CreatedDate * 1000).toISOString()
      : undefined,
    lastAccessedDate: v.LastAccessedDate
      ? new Date(v.LastAccessedDate * 1000).toISOString()
      : undefined,
  }));
}

export async function putSecretValue(
  secretId: string,
  secretString: string,
): Promise<void> {
  await smRequest("PutSecretValue", {
    SecretId: secretId,
    SecretString: secretString,
  });
}

export async function createSecret(
  name: string,
  secretString: string,
  description?: string,
): Promise<{ arn: string }> {
  const body: Record<string, string> = {
    Name: name,
    SecretString: secretString,
  };
  if (description) body.Description = description;
  const data = (await smRequest("CreateSecret", body)) as { ARN?: string };
  return { arn: data.ARN ?? "" };
}

export async function deleteSecret(
  secretId: string,
  forceDelete = false,
): Promise<void> {
  const body: Record<string, unknown> = { SecretId: secretId };
  if (forceDelete) body.ForceDeleteWithoutRecovery = true;
  await smRequest("DeleteSecret", body);
}
