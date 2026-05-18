/**
 * AWS Organizations API client.
 *
 * AWSim uses the JSON-1.1 protocol with `X-Amz-Target:
 * AWSOrganizationsV20161128.<Action>`. Shapes are normalised to
 * camel-cased forms used directly by the UI.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "organizations";
const TARGET_PREFIX = "AWSOrganizationsV20161128";

async function orgRequest<T>(action: string, body: unknown = {}): Promise<T> {
  const res = await loggedFetch(SERVICE, action, "POST", `${ENDPOINT}/`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `${TARGET_PREFIX}.${action}`,
      Authorization: authHeader(SERVICE),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`HTTP ${res.status}: ${text || res.statusText}`);
  }
  const text = await res.text();
  return text ? (JSON.parse(text) as T) : ({} as T);
}

// -- Types --

export interface Account {
  id: string;
  arn: string;
  email: string;
  name: string;
  status: string;
  joinedMethod?: string;
  joinedTimestamp?: number;
}

export interface OrganizationalUnit {
  id: string;
  arn: string;
  name: string;
  parentId?: string;
}

export interface Root {
  id: string;
  arn: string;
  name: string;
  policyTypes: { type: string; status: string }[];
}

export interface Policy {
  id: string;
  arn: string;
  name: string;
  description?: string;
  type: string;
  awsManaged: boolean;
}

export interface PolicyDocument {
  id: string;
  name: string;
  description?: string;
  type: string;
  awsManaged: boolean;
  content: string;
}

// -- Raw shapes --

interface RawAccount {
  Id?: string;
  Arn?: string;
  Email?: string;
  Name?: string;
  Status?: string;
  JoinedMethod?: string;
  JoinedTimestamp?: number;
}

interface RawOu {
  Id?: string;
  Arn?: string;
  Name?: string;
}

interface RawRoot {
  Id?: string;
  Arn?: string;
  Name?: string;
  PolicyTypes?: { Type?: string; Status?: string }[];
}

interface RawPolicy {
  Id?: string;
  Arn?: string;
  Name?: string;
  Description?: string;
  Type?: string;
  AwsManaged?: boolean;
}

// -- Operations --

export async function listAccounts(): Promise<{ accounts: Account[] }> {
  const data = await orgRequest<{ Accounts?: RawAccount[] }>("ListAccounts");
  return {
    accounts: (data.Accounts ?? []).map((a) => ({
      id: a.Id ?? "",
      arn: a.Arn ?? "",
      email: a.Email ?? "",
      name: a.Name ?? "",
      status: a.Status ?? "",
      joinedMethod: a.JoinedMethod,
      joinedTimestamp: a.JoinedTimestamp,
    })),
  };
}

export async function describeAccount(
  accountId: string,
): Promise<Account | null> {
  const data = await orgRequest<{ Account?: RawAccount }>("DescribeAccount", {
    AccountId: accountId,
  });
  if (!data.Account) return null;
  return {
    id: data.Account.Id ?? "",
    arn: data.Account.Arn ?? "",
    email: data.Account.Email ?? "",
    name: data.Account.Name ?? "",
    status: data.Account.Status ?? "",
    joinedMethod: data.Account.JoinedMethod,
    joinedTimestamp: data.Account.JoinedTimestamp,
  };
}

export async function listRoots(): Promise<{ roots: Root[] }> {
  const data = await orgRequest<{ Roots?: RawRoot[] }>("ListRoots");
  return {
    roots: (data.Roots ?? []).map((r) => ({
      id: r.Id ?? "",
      arn: r.Arn ?? "",
      name: r.Name ?? "",
      policyTypes: (r.PolicyTypes ?? []).map((p) => ({
        type: p.Type ?? "",
        status: p.Status ?? "",
      })),
    })),
  };
}

export async function listOrganizationalUnitsForParent(
  parentId: string,
): Promise<{ ous: OrganizationalUnit[] }> {
  const data = await orgRequest<{ OrganizationalUnits?: RawOu[] }>(
    "ListOrganizationalUnitsForParent",
    { ParentId: parentId },
  );
  return {
    ous: (data.OrganizationalUnits ?? []).map((o) => ({
      id: o.Id ?? "",
      arn: o.Arn ?? "",
      name: o.Name ?? "",
      parentId,
    })),
  };
}

export async function describeOrganizationalUnit(
  ouId: string,
): Promise<OrganizationalUnit | null> {
  const data = await orgRequest<{ OrganizationalUnit?: RawOu }>(
    "DescribeOrganizationalUnit",
    { OrganizationalUnitId: ouId },
  );
  if (!data.OrganizationalUnit) return null;
  return {
    id: data.OrganizationalUnit.Id ?? "",
    arn: data.OrganizationalUnit.Arn ?? "",
    name: data.OrganizationalUnit.Name ?? "",
  };
}

export async function listPolicies(
  filter: string = "SERVICE_CONTROL_POLICY",
): Promise<{ policies: Policy[] }> {
  const data = await orgRequest<{ Policies?: RawPolicy[] }>("ListPolicies", {
    Filter: filter,
  });
  return {
    policies: (data.Policies ?? []).map((p) => ({
      id: p.Id ?? "",
      arn: p.Arn ?? "",
      name: p.Name ?? "",
      description: p.Description,
      type: p.Type ?? "",
      awsManaged: !!p.AwsManaged,
    })),
  };
}

export async function createPolicy(
  name: string,
  description: string,
  content: string,
  type: string = "SERVICE_CONTROL_POLICY",
): Promise<Policy> {
  const data = await orgRequest<{ Policy?: { PolicySummary?: RawPolicy } }>(
    "CreatePolicy",
    { Name: name, Description: description, Content: content, Type: type },
  );
  const s = data.Policy?.PolicySummary ?? {};
  return {
    id: s.Id ?? "",
    arn: s.Arn ?? "",
    name: s.Name ?? "",
    description: s.Description,
    type: s.Type ?? type,
    awsManaged: !!s.AwsManaged,
  };
}

export async function describePolicy(
  policyId: string,
): Promise<PolicyDocument | null> {
  const data = await orgRequest<{
    Policy?: { PolicySummary?: RawPolicy; Content?: string };
  }>("DescribePolicy", { PolicyId: policyId });
  if (!data.Policy) return null;
  const s = data.Policy.PolicySummary ?? {};
  return {
    id: s.Id ?? "",
    name: s.Name ?? "",
    description: s.Description,
    type: s.Type ?? "",
    awsManaged: !!s.AwsManaged,
    content: data.Policy.Content ?? "",
  };
}

export async function attachPolicy(
  policyId: string,
  targetId: string,
): Promise<void> {
  await orgRequest("AttachPolicy", { PolicyId: policyId, TargetId: targetId });
}

export async function detachPolicy(
  policyId: string,
  targetId: string,
): Promise<void> {
  await orgRequest("DetachPolicy", { PolicyId: policyId, TargetId: targetId });
}

// -- Helpers --

export function accountStatusVariant(
  status: string,
): "default" | "secondary" | "destructive" | "outline" {
  if (status === "ACTIVE") return "default";
  if (status === "SUSPENDED") return "destructive";
  return "outline";
}
