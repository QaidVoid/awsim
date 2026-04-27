/**
 * Typed STS (Security Token Service) API client.
 *
 * Uses the AWS query protocol (form-encoded) at the root endpoint.
 * Names map directly to AWS SDK STS operations.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "sts";
const VERSION = "2011-06-15";

// ---------- Types ----------

export interface CallerIdentity {
  account: string;
  arn: string;
  userId: string;
}

export interface Credentials {
  accessKeyId: string;
  secretAccessKey: string;
  sessionToken: string;
  expiration: string;
}

export interface AssumedRoleUser {
  assumedRoleId: string;
  arn: string;
}

export interface AssumeRoleResponse {
  credentials: Credentials;
  assumedRoleUser?: AssumedRoleUser;
  packedPolicySize?: number;
}

export interface FederationToken {
  credentials: Credentials;
  federatedUser: { federatedUserId: string; arn: string };
  packedPolicySize?: number;
}

export interface AccessKeyInfo {
  account: string;
}

// ---------- Internal request ----------

function getNode(parent: Element | Document, tag: string): Element | null {
  return parent.getElementsByTagName(tag)[0] ?? null;
}

function getText(parent: Element | Document, tag: string): string {
  return getNode(parent, tag)?.textContent?.trim() ?? "";
}

async function request(
  action: string,
  params: Record<string, string> = {},
): Promise<Document> {
  const body = new URLSearchParams({
    Action: action,
    Version: VERSION,
    ...params,
  });
  const res = await loggedFetch(SERVICE, action, "POST", ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-www-form-urlencoded",
      Authorization: authHeader(SERVICE),
      "X-Amz-Date": amzDate(),
    },
    body: body.toString(),
  });
  const text = await res.text();
  if (!res.ok) {
    throw new Error(`STS ${action} failed (HTTP ${res.status}): ${text}`);
  }
  return new DOMParser().parseFromString(text, "text/xml");
}

function parseCredentials(node: Element): Credentials {
  return {
    accessKeyId: getText(node, "AccessKeyId"),
    secretAccessKey: getText(node, "SecretAccessKey"),
    sessionToken: getText(node, "SessionToken"),
    expiration: getText(node, "Expiration"),
  };
}

// ---------- Operations ----------

export async function getCallerIdentity(): Promise<CallerIdentity> {
  const doc = await request("GetCallerIdentity");
  return {
    account: getText(doc, "Account"),
    arn: getText(doc, "Arn"),
    userId: getText(doc, "UserId"),
  };
}

export interface AssumeRoleInput {
  roleArn: string;
  roleSessionName: string;
  durationSeconds?: number;
  externalId?: string;
  policy?: string;
}

export async function assumeRole(
  input: AssumeRoleInput,
): Promise<AssumeRoleResponse> {
  const params: Record<string, string> = {
    RoleArn: input.roleArn,
    RoleSessionName: input.roleSessionName,
  };
  if (input.durationSeconds)
    params["DurationSeconds"] = String(input.durationSeconds);
  if (input.externalId) params["ExternalId"] = input.externalId;
  if (input.policy) params["Policy"] = input.policy;
  const doc = await request("AssumeRole", params);
  const credsNode = getNode(doc, "Credentials");
  const userNode = getNode(doc, "AssumedRoleUser");
  const packed = getText(doc, "PackedPolicySize");
  return {
    credentials: credsNode
      ? parseCredentials(credsNode)
      : {
          accessKeyId: "",
          secretAccessKey: "",
          sessionToken: "",
          expiration: "",
        },
    assumedRoleUser: userNode
      ? {
          assumedRoleId: getText(userNode, "AssumedRoleId"),
          arn: getText(userNode, "Arn"),
        }
      : undefined,
    packedPolicySize: packed ? Number(packed) : undefined,
  };
}

export interface AssumeRoleWithSamlInput {
  roleArn: string;
  principalArn: string;
  samlAssertion: string;
  durationSeconds?: number;
}

export async function assumeRoleWithSAML(
  input: AssumeRoleWithSamlInput,
): Promise<AssumeRoleResponse> {
  const params: Record<string, string> = {
    RoleArn: input.roleArn,
    PrincipalArn: input.principalArn,
    SAMLAssertion: input.samlAssertion,
  };
  if (input.durationSeconds)
    params["DurationSeconds"] = String(input.durationSeconds);
  const doc = await request("AssumeRoleWithSAML", params);
  const credsNode = getNode(doc, "Credentials");
  return {
    credentials: credsNode
      ? parseCredentials(credsNode)
      : {
          accessKeyId: "",
          secretAccessKey: "",
          sessionToken: "",
          expiration: "",
        },
  };
}

export interface AssumeRoleWithWebIdentityInput {
  roleArn: string;
  roleSessionName: string;
  webIdentityToken: string;
  providerId?: string;
  durationSeconds?: number;
}

export async function assumeRoleWithWebIdentity(
  input: AssumeRoleWithWebIdentityInput,
): Promise<AssumeRoleResponse> {
  const params: Record<string, string> = {
    RoleArn: input.roleArn,
    RoleSessionName: input.roleSessionName,
    WebIdentityToken: input.webIdentityToken,
  };
  if (input.providerId) params["ProviderId"] = input.providerId;
  if (input.durationSeconds)
    params["DurationSeconds"] = String(input.durationSeconds);
  const doc = await request("AssumeRoleWithWebIdentity", params);
  const credsNode = getNode(doc, "Credentials");
  return {
    credentials: credsNode
      ? parseCredentials(credsNode)
      : {
          accessKeyId: "",
          secretAccessKey: "",
          sessionToken: "",
          expiration: "",
        },
  };
}

export interface FederationTokenInput {
  name: string;
  policy?: string;
  durationSeconds?: number;
}

export async function getFederationToken(
  input: FederationTokenInput,
): Promise<FederationToken> {
  const params: Record<string, string> = { Name: input.name };
  if (input.policy) params["Policy"] = input.policy;
  if (input.durationSeconds)
    params["DurationSeconds"] = String(input.durationSeconds);
  const doc = await request("GetFederationToken", params);
  const credsNode = getNode(doc, "Credentials");
  const userNode = getNode(doc, "FederatedUser");
  return {
    credentials: credsNode
      ? parseCredentials(credsNode)
      : {
          accessKeyId: "",
          secretAccessKey: "",
          sessionToken: "",
          expiration: "",
        },
    federatedUser: {
      federatedUserId: userNode ? getText(userNode, "FederatedUserId") : "",
      arn: userNode ? getText(userNode, "Arn") : "",
    },
  };
}

export async function getSessionToken(
  durationSeconds?: number,
): Promise<{ credentials: Credentials }> {
  const params: Record<string, string> = {};
  if (durationSeconds) params["DurationSeconds"] = String(durationSeconds);
  const doc = await request("GetSessionToken", params);
  const credsNode = getNode(doc, "Credentials");
  return {
    credentials: credsNode
      ? parseCredentials(credsNode)
      : {
          accessKeyId: "",
          secretAccessKey: "",
          sessionToken: "",
          expiration: "",
        },
  };
}

export async function decodeAuthorizationMessage(
  encodedMessage: string,
): Promise<string> {
  const doc = await request("DecodeAuthorizationMessage", {
    EncodedMessage: encodedMessage,
  });
  return getText(doc, "DecodedMessage");
}

export async function getAccessKeyInfo(
  accessKeyId: string,
): Promise<AccessKeyInfo> {
  const doc = await request("GetAccessKeyInfo", { AccessKeyId: accessKeyId });
  return { account: getText(doc, "Account") };
}
