/**
 * Typed MemoryDB for Redis API client. AwsJson1.1 — X-Amz-Target prefix
 * is `AmazonMemoryDB`.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "memorydb";
const TARGET_PREFIX = "AmazonMemoryDB";

export interface ClusterEndpoint {
  address: string;
  port: number;
}

export interface Cluster {
  name: string;
  arn: string;
  status: string;
  nodeType: string;
  engineVersion: string;
  parameterGroupName: string;
  subnetGroupName: string;
  aclName: string;
  numberOfShards: number;
  tlsEnabled: boolean;
  clusterEndpoint: ClusterEndpoint;
  description?: string;
}

export interface User {
  name: string;
  arn: string;
  status: string;
  accessString: string;
  authentication: { type: string };
}

export interface Acl {
  name: string;
  arn: string;
  status: string;
  userNames: string[];
}

async function request<T>(
  action: string,
  body: Record<string, unknown> = {},
): Promise<T> {
  const res = await loggedFetch(SERVICE, action, "POST", ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `${TARGET_PREFIX}.${action}`,
      Authorization: authHeader(SERVICE),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(body),
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
    throw new Error(`MemoryDB ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawCluster {
  Name: string;
  ARN: string;
  Status: string;
  NodeType: string;
  EngineVersion: string;
  ParameterGroupName: string;
  SubnetGroupName: string;
  ACLName: string;
  NumberOfShards: number;
  TLSEnabled: boolean;
  ClusterEndpoint?: { Address?: string; Port?: number };
  Description?: string;
}

interface RawUser {
  Name: string;
  ARN: string;
  Status: string;
  AccessString: string;
  Authentication?: { Type?: string };
}

interface RawAcl {
  Name: string;
  ARN: string;
  Status: string;
  UserNames?: string[];
}

const fromCluster = (r: RawCluster): Cluster => ({
  name: r.Name,
  arn: r.ARN,
  status: r.Status,
  nodeType: r.NodeType,
  engineVersion: r.EngineVersion,
  parameterGroupName: r.ParameterGroupName,
  subnetGroupName: r.SubnetGroupName,
  aclName: r.ACLName,
  numberOfShards: r.NumberOfShards,
  tlsEnabled: r.TLSEnabled,
  clusterEndpoint: {
    address: r.ClusterEndpoint?.Address ?? "",
    port: r.ClusterEndpoint?.Port ?? 6379,
  },
  description: r.Description,
});

const fromUser = (r: RawUser): User => ({
  name: r.Name,
  arn: r.ARN,
  status: r.Status,
  accessString: r.AccessString,
  authentication: { type: r.Authentication?.Type ?? "password" },
});

const fromAcl = (r: RawAcl): Acl => ({
  name: r.Name,
  arn: r.ARN,
  status: r.Status,
  userNames: r.UserNames ?? [],
});

// ---------- Clusters ----------
export async function describeClusters(): Promise<Cluster[]> {
  const data = await request<{ Clusters?: RawCluster[] }>("DescribeClusters");
  return (data.Clusters ?? []).map(fromCluster);
}

export async function createCluster(input: {
  clusterName: string;
  nodeType: string;
  aclName: string;
  numShards?: number;
}): Promise<Cluster> {
  const r = await request<{ Cluster: RawCluster }>("CreateCluster", {
    ClusterName: input.clusterName,
    NodeType: input.nodeType,
    ACLName: input.aclName,
    NumShards: input.numShards ?? 1,
  });
  return fromCluster(r.Cluster);
}

export async function deleteCluster(name: string): Promise<void> {
  await request<unknown>("DeleteCluster", { ClusterName: name });
}

// ---------- Users ----------
export async function describeUsers(): Promise<User[]> {
  const data = await request<{ Users?: RawUser[] }>("DescribeUsers");
  return (data.Users ?? []).map(fromUser);
}

export async function createUser(
  userName: string,
  accessString = "on ~* +@all",
): Promise<User> {
  const r = await request<{ User: RawUser }>("CreateUser", {
    UserName: userName,
    AccessString: accessString,
    AuthenticationMode: { Type: "password", Passwords: ["dummypassword12345"] },
  });
  return fromUser(r.User);
}

export async function deleteUser(name: string): Promise<void> {
  await request<unknown>("DeleteUser", { UserName: name });
}

// ---------- ACLs ----------
export async function describeAcls(): Promise<Acl[]> {
  const data = await request<{ ACLs?: RawAcl[] }>("DescribeACLs");
  return (data.ACLs ?? []).map(fromAcl);
}

export async function createAcl(
  aclName: string,
  userNames: string[] = [],
): Promise<Acl> {
  const r = await request<{ ACL: RawAcl }>("CreateACL", {
    ACLName: aclName,
    UserNames: userNames,
  });
  return fromAcl(r.ACL);
}

export async function deleteAcl(name: string): Promise<void> {
  await request<unknown>("DeleteACL", { ACLName: name });
}
