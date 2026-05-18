/**
 * Typed KMS API client.
 *
 * Talks to the AWSim emulator using the TrentService JSON 1.0 protocol.
 */

const ENDPOINT = "http://localhost:4566";
const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, "");

function authHeader(): string {
  return `AWS4-HMAC-SHA256 Credential=awsim-admin/${FAKE_DATE}/us-east-1/kms/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

function amzDate(): string {
  return new Date().toISOString().replace(/[:-]/g, "").slice(0, 15) + "Z";
}

async function kmsRequest(
  action: string,
  body: unknown = {},
): Promise<unknown> {
  const res = await fetch(ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.0",
      "X-Amz-Target": `TrentService.${action}`,
      Authorization: authHeader(),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`KMS ${action} failed: ${res.status} ${text}`);
  return text ? JSON.parse(text) : {};
}

// ---- Types ----

export interface Key {
  keyId: string;
  keyArn: string;
}

export interface KeyDetail extends Key {
  description: string;
  keyState: string;
  keyUsage?: string;
  origin?: string;
  enabled: boolean;
  creationDate: string;
}

export interface Alias {
  aliasName: string;
  aliasArn: string;
  targetKeyId: string;
}

// ---- Operations ----

export async function listKeys(): Promise<Key[]> {
  const data = (await kmsRequest("ListKeys")) as {
    Keys?: { KeyId: string; KeyArn: string }[];
  };
  return (data.Keys ?? []).map((k) => ({
    keyId: k.KeyId,
    keyArn: k.KeyArn,
  }));
}

export async function describeKey(keyId: string): Promise<KeyDetail> {
  const data = (await kmsRequest("DescribeKey", { KeyId: keyId })) as {
    KeyMetadata?: {
      KeyId: string;
      Arn: string;
      Description?: string;
      KeyState: string;
      KeyUsage?: string;
      Origin?: string;
      Enabled?: boolean;
      CreationDate: number;
    };
  };
  const k = data.KeyMetadata ?? ({} as NonNullable<typeof data.KeyMetadata>);
  return {
    keyId: k.KeyId ?? keyId,
    keyArn: k.Arn ?? "",
    description: k.Description ?? "",
    keyState: k.KeyState ?? "",
    keyUsage: k.KeyUsage,
    origin: k.Origin,
    enabled: k.Enabled ?? true,
    creationDate: k.CreationDate
      ? new Date(k.CreationDate * 1000).toISOString()
      : "",
  };
}

export async function createKey(description?: string): Promise<Key> {
  const body: Record<string, unknown> = { KeyUsage: "ENCRYPT_DECRYPT" };
  if (description) body.Description = description;
  const data = (await kmsRequest("CreateKey", body)) as {
    KeyMetadata?: { KeyId?: string; Arn?: string };
  };
  return {
    keyId: data.KeyMetadata?.KeyId ?? "",
    keyArn: data.KeyMetadata?.Arn ?? "",
  };
}

export async function scheduleKeyDeletion(
  keyId: string,
  pendingWindowInDays = 7,
): Promise<void> {
  await kmsRequest("ScheduleKeyDeletion", {
    KeyId: keyId,
    PendingWindowInDays: pendingWindowInDays,
  });
}

export async function listAliases(): Promise<Alias[]> {
  const data = (await kmsRequest("ListAliases")) as {
    Aliases?: { AliasName: string; AliasArn: string; TargetKeyId?: string }[];
  };
  return (data.Aliases ?? []).map((a) => ({
    aliasName: a.AliasName,
    aliasArn: a.AliasArn,
    targetKeyId: a.TargetKeyId ?? "",
  }));
}

export async function createAlias(
  aliasName: string,
  targetKeyId: string,
): Promise<void> {
  const name = aliasName.startsWith("alias/") ? aliasName : `alias/${aliasName}`;
  await kmsRequest("CreateAlias", {
    AliasName: name,
    TargetKeyId: targetKeyId,
  });
}

export async function deleteAlias(aliasName: string): Promise<void> {
  await kmsRequest("DeleteAlias", { AliasName: aliasName });
}

export async function getKeyPolicy(
  keyId: string,
  policyName = "default",
): Promise<string> {
  const data = (await kmsRequest("GetKeyPolicy", {
    KeyId: keyId,
    PolicyName: policyName,
  })) as { Policy?: string };
  return data.Policy ?? "";
}

export async function putKeyPolicy(
  keyId: string,
  policy: string,
  policyName = "default",
): Promise<void> {
  await kmsRequest("PutKeyPolicy", {
    KeyId: keyId,
    PolicyName: policyName,
    Policy: policy,
  });
}

export async function encrypt(
  keyId: string,
  plaintext: string,
): Promise<string> {
  const encoded = btoa(plaintext);
  const data = (await kmsRequest("Encrypt", {
    KeyId: keyId,
    Plaintext: encoded,
  })) as { CiphertextBlob: string };
  return data.CiphertextBlob ?? "";
}

export async function decrypt(ciphertextBlob: string): Promise<string> {
  const data = (await kmsRequest("Decrypt", {
    CiphertextBlob: ciphertextBlob,
  })) as { Plaintext: string };
  try {
    return atob(data.Plaintext ?? "");
  } catch {
    return data.Plaintext ?? "";
  }
}

export async function generateDataKey(
  keyId: string,
  keySpec: "AES_128" | "AES_256" = "AES_256",
): Promise<{ ciphertextBlob: string; plaintext: string }> {
  const data = (await kmsRequest("GenerateDataKey", {
    KeyId: keyId,
    KeySpec: keySpec,
  })) as { CiphertextBlob: string; Plaintext: string };
  return {
    ciphertextBlob: data.CiphertextBlob ?? "",
    plaintext: data.Plaintext ?? "",
  };
}

export async function enableKeyRotation(keyId: string): Promise<void> {
  await kmsRequest("EnableKeyRotation", { KeyId: keyId });
}

export async function disableKeyRotation(keyId: string): Promise<void> {
  await kmsRequest("DisableKeyRotation", { KeyId: keyId });
}

export async function getKeyRotationStatus(keyId: string): Promise<boolean> {
  try {
    const data = (await kmsRequest("GetKeyRotationStatus", {
      KeyId: keyId,
    })) as {
      KeyRotationEnabled?: boolean;
    };
    return data.KeyRotationEnabled ?? false;
  } catch {
    return false;
  }
}
