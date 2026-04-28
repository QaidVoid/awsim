/**
 * Typed SSO Admin (IAM Identity Center) API client.
 *
 * Wraps the AWS JSON 1.1 SWBExternalService API used for
 * `sso-admin`. Names map to AWS SDK SSO Admin operations.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "sso";
const TARGET_PREFIX = "SWBExternalService";

// ---------- Types ----------

export interface Instance {
  instanceArn: string;
  identityStoreId: string;
  name?: string;
  status?: string;
}

export interface PermissionSet {
  permissionSetArn: string;
  name: string;
  description?: string;
  sessionDuration?: string;
  relayState?: string;
  createdDate?: string;
}

export interface AccountAssignment {
  accountId: string;
  permissionSetArn: string;
  principalId: string;
  principalType: string;
}

export interface ManagedPolicy {
  arn: string;
  name?: string;
}

export interface InlinePolicy {
  policyDocument: string;
}

// ---------- Internal request ----------

async function request<T>(
  action: string,
  params: Record<string, unknown> = {},
): Promise<T> {
  const res = await loggedFetch(SERVICE, action, "POST", ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `${TARGET_PREFIX}.${action}`,
      Authorization: authHeader(SERVICE),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(params),
  });
  const text = await res.text();
  if (!res.ok) {
    let msg = text;
    try {
      const data = JSON.parse(text) as { message?: string; Message?: string };
      msg = data.message ?? data.Message ?? text;
    } catch {
      // not JSON
    }
    throw new Error(`SSO Admin ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

// ---------- Operations ----------

export async function listInstances(): Promise<Instance[]> {
  const data = await request<{
    Instances?: {
      InstanceArn: string;
      IdentityStoreId: string;
      Name?: string;
      Status?: string;
    }[];
  }>("ListInstances");
  return (data.Instances ?? []).map((i) => ({
    instanceArn: i.InstanceArn,
    identityStoreId: i.IdentityStoreId,
    name: i.Name,
    status: i.Status,
  }));
}

export async function listPermissionSets(
  instanceArn: string,
): Promise<string[]> {
  const data = await request<{ PermissionSets?: string[] }>(
    "ListPermissionSets",
    { InstanceArn: instanceArn },
  );
  return data.PermissionSets ?? [];
}

export async function describePermissionSet(
  instanceArn: string,
  permissionSetArn: string,
): Promise<PermissionSet> {
  const data = await request<{
    PermissionSet?: {
      PermissionSetArn: string;
      Name: string;
      Description?: string;
      SessionDuration?: string;
      RelayState?: string;
      CreatedDate?: string;
    };
  }>("DescribePermissionSet", {
    InstanceArn: instanceArn,
    PermissionSetArn: permissionSetArn,
  });
  const ps = data.PermissionSet;
  return {
    permissionSetArn: ps?.PermissionSetArn ?? permissionSetArn,
    name: ps?.Name ?? "",
    description: ps?.Description,
    sessionDuration: ps?.SessionDuration,
    relayState: ps?.RelayState,
    createdDate: ps?.CreatedDate,
  };
}

export async function listAccountAssignments(
  instanceArn: string,
  accountId: string,
  permissionSetArn: string,
): Promise<AccountAssignment[]> {
  const data = await request<{
    AccountAssignments?: {
      AccountId: string;
      PermissionSetArn: string;
      PrincipalId: string;
      PrincipalType: string;
    }[];
  }>("ListAccountAssignments", {
    InstanceArn: instanceArn,
    AccountId: accountId,
    PermissionSetArn: permissionSetArn,
  });
  return (data.AccountAssignments ?? []).map((a) => ({
    accountId: a.AccountId,
    permissionSetArn: a.PermissionSetArn,
    principalId: a.PrincipalId,
    principalType: a.PrincipalType,
  }));
}

export async function listManagedPoliciesInPermissionSet(
  instanceArn: string,
  permissionSetArn: string,
): Promise<ManagedPolicy[]> {
  const data = await request<{
    AttachedManagedPolicies?: { Arn: string; Name?: string }[];
  }>("ListManagedPoliciesInPermissionSet", {
    InstanceArn: instanceArn,
    PermissionSetArn: permissionSetArn,
  });
  return (data.AttachedManagedPolicies ?? []).map((p) => ({
    arn: p.Arn,
    name: p.Name,
  }));
}

export async function getInlinePolicyForPermissionSet(
  instanceArn: string,
  permissionSetArn: string,
): Promise<InlinePolicy | null> {
  try {
    const data = await request<{ InlinePolicy?: string }>(
      "GetInlinePolicyForPermissionSet",
      {
        InstanceArn: instanceArn,
        PermissionSetArn: permissionSetArn,
      },
    );
    if (!data.InlinePolicy) return null;
    return { policyDocument: data.InlinePolicy };
  } catch {
    return null;
  }
}

export async function deletePermissionSet(
  instanceArn: string,
  permissionSetArn: string,
): Promise<void> {
  await request("DeletePermissionSet", {
    InstanceArn: instanceArn,
    PermissionSetArn: permissionSetArn,
  });
}
