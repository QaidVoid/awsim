/**
 * Typed WAF v2 API client.
 *
 * Both REGIONAL and CLOUDFRONT scopes are supported.
 */

const ENDPOINT = "http://localhost:4566";
const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, "");

function authHeader(): string {
  return `AWS4-HMAC-SHA256 Credential=awsim-admin/${FAKE_DATE}/us-east-1/wafv2/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

function amzDate(): string {
  return new Date().toISOString().replace(/[:-]/g, "").slice(0, 15) + "Z";
}

async function wafRequest(
  action: string,
  body: unknown = {},
): Promise<unknown> {
  const res = await fetch(ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `AWSWAF_20190729.${action}`,
      Authorization: authHeader(),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`WAF ${action} failed: ${res.status} ${text}`);
  return text ? JSON.parse(text) : {};
}

// ---- Types ----

export type WafScope = "REGIONAL" | "CLOUDFRONT";

export interface WebAcl {
  id: string;
  name: string;
  arn: string;
  description?: string;
  lockToken: string;
}

export interface WebAclDetail extends WebAcl {
  defaultAction?: string;
  rules?: { name: string; priority: number; action: string }[];
  capacity?: number;
}

export interface RuleGroup {
  id: string;
  name: string;
  arn: string;
  description?: string;
  lockToken: string;
}

export interface RuleGroupDetail extends RuleGroup {
  capacity?: number;
  rules?: { name: string; priority: number; action: string }[];
}

export interface IpSet {
  id: string;
  name: string;
  arn: string;
  description?: string;
  lockToken: string;
}

export interface IpSetDetail extends IpSet {
  ipAddressVersion?: "IPV4" | "IPV6";
  addresses: string[];
}

// ---- Web ACLs ----

export async function listWebAcls(scope: WafScope): Promise<WebAcl[]> {
  const data = (await wafRequest("ListWebACLs", { Scope: scope })) as {
    WebACLs?: {
      Id: string;
      Name: string;
      ARN: string;
      Description?: string;
      LockToken: string;
    }[];
  };
  return (data.WebACLs ?? []).map((w) => ({
    id: w.Id,
    name: w.Name,
    arn: w.ARN,
    description: w.Description,
    lockToken: w.LockToken,
  }));
}

export async function getWebAcl(
  name: string,
  id: string,
  scope: WafScope,
): Promise<WebAclDetail> {
  const data = (await wafRequest("GetWebACL", {
    Name: name,
    Id: id,
    Scope: scope,
  })) as {
    WebACL?: {
      Id: string;
      Name: string;
      ARN: string;
      Description?: string;
      DefaultAction?: { Allow?: unknown; Block?: unknown };
      Rules?: {
        Name: string;
        Priority: number;
        Action?: { Allow?: unknown; Block?: unknown; Count?: unknown };
      }[];
      Capacity?: number;
    };
    LockToken?: string;
  };
  const w = data.WebACL ?? ({} as NonNullable<typeof data.WebACL>);
  let defaultAction = "—";
  if (w.DefaultAction?.Allow) defaultAction = "Allow";
  else if (w.DefaultAction?.Block) defaultAction = "Block";
  return {
    id: w.Id ?? id,
    name: w.Name ?? name,
    arn: w.ARN ?? "",
    description: w.Description,
    lockToken: data.LockToken ?? "",
    defaultAction,
    capacity: w.Capacity,
    rules: (w.Rules ?? []).map((r) => ({
      name: r.Name,
      priority: r.Priority,
      action: r.Action?.Allow
        ? "Allow"
        : r.Action?.Block
          ? "Block"
          : r.Action?.Count
            ? "Count"
            : "—",
    })),
  };
}

// ---- Rule Groups ----

export async function listRuleGroups(scope: WafScope): Promise<RuleGroup[]> {
  const data = (await wafRequest("ListRuleGroups", { Scope: scope })) as {
    RuleGroups?: {
      Id: string;
      Name: string;
      ARN: string;
      Description?: string;
      LockToken: string;
    }[];
  };
  return (data.RuleGroups ?? []).map((g) => ({
    id: g.Id,
    name: g.Name,
    arn: g.ARN,
    description: g.Description,
    lockToken: g.LockToken,
  }));
}

export async function getRuleGroup(
  name: string,
  id: string,
  scope: WafScope,
): Promise<RuleGroupDetail> {
  const data = (await wafRequest("GetRuleGroup", {
    Name: name,
    Id: id,
    Scope: scope,
  })) as {
    RuleGroup?: {
      Id: string;
      Name: string;
      ARN: string;
      Description?: string;
      Capacity?: number;
      Rules?: {
        Name: string;
        Priority: number;
        Action?: { Allow?: unknown; Block?: unknown; Count?: unknown };
      }[];
    };
    LockToken?: string;
  };
  const g = data.RuleGroup ?? ({} as NonNullable<typeof data.RuleGroup>);
  return {
    id: g.Id ?? id,
    name: g.Name ?? name,
    arn: g.ARN ?? "",
    description: g.Description,
    lockToken: data.LockToken ?? "",
    capacity: g.Capacity,
    rules: (g.Rules ?? []).map((r) => ({
      name: r.Name,
      priority: r.Priority,
      action: r.Action?.Allow
        ? "Allow"
        : r.Action?.Block
          ? "Block"
          : r.Action?.Count
            ? "Count"
            : "—",
    })),
  };
}

// ---- IP Sets ----

export async function listIpSets(scope: WafScope): Promise<IpSet[]> {
  const data = (await wafRequest("ListIPSets", { Scope: scope })) as {
    IPSets?: {
      Id: string;
      Name: string;
      ARN: string;
      Description?: string;
      LockToken: string;
    }[];
  };
  return (data.IPSets ?? []).map((s) => ({
    id: s.Id,
    name: s.Name,
    arn: s.ARN,
    description: s.Description,
    lockToken: s.LockToken,
  }));
}

export async function getIpSet(
  name: string,
  id: string,
  scope: WafScope,
): Promise<IpSetDetail> {
  const data = (await wafRequest("GetIPSet", {
    Name: name,
    Id: id,
    Scope: scope,
  })) as {
    IPSet?: {
      Id: string;
      Name: string;
      ARN: string;
      Description?: string;
      IPAddressVersion?: "IPV4" | "IPV6";
      Addresses?: string[];
    };
    LockToken?: string;
  };
  const s = data.IPSet ?? ({} as NonNullable<typeof data.IPSet>);
  return {
    id: s.Id ?? id,
    name: s.Name ?? name,
    arn: s.ARN ?? "",
    description: s.Description,
    lockToken: data.LockToken ?? "",
    ipAddressVersion: s.IPAddressVersion,
    addresses: s.Addresses ?? [],
  };
}
