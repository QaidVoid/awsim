/**
 * EKS API client.
 *
 * Wraps the AWSim EKS REST endpoints
 * (`/clusters`, `/clusters/{name}`, `/clusters/{name}/node-groups`,
 * `/clusters/{name}/fargate-profiles`).
 *
 * All payloads are normalised to camel-cased shapes for the UI.
 */

const ENDPOINT = "http://localhost:4566";
const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, "");

function authHeader(): string {
  return `AWS4-HMAC-SHA256 Credential=awsim-admin/${FAKE_DATE}/us-east-1/eks/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

function amzDate(): string {
  return new Date().toISOString().replace(/[:-]/g, "").slice(0, 15) + "Z";
}

function eksHeaders(): Record<string, string> {
  return {
    "Content-Type": "application/json",
    Authorization: authHeader(),
    "X-Amz-Date": amzDate(),
  };
}

async function eksFetch<T>(path: string, init: RequestInit = {}): Promise<T> {
  const res = await fetch(`${ENDPOINT}${path}`, {
    headers: eksHeaders(),
    ...init,
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`HTTP ${res.status}: ${text || res.statusText}`);
  }
  const text = await res.text();
  return text ? (JSON.parse(text) as T) : ({} as T);
}

// -- Types --

export interface Cluster {
  name: string;
  arn: string;
  status: string;
  version: string;
  endpoint: string;
  roleArn: string;
  createdAt: string;
}

export interface Nodegroup {
  name: string;
  arn: string;
  clusterName: string;
  status: string;
  capacityType: string;
  instanceTypes: string[];
  diskSize: number;
  desiredSize: number;
  minSize: number;
  maxSize: number;
  amiType: string;
  createdAt: string;
}

export interface FargateProfile {
  name: string;
  arn: string;
  clusterName: string;
  status: string;
  podExecutionRoleArn: string;
  selectors: { namespace: string; labels?: Record<string, string> }[];
  createdAt: string;
}

interface RawCluster {
  name?: string;
  arn?: string;
  status?: string;
  version?: string;
  endpoint?: string;
  roleArn?: string;
  createdAt?: string | number;
}

interface RawNodegroup {
  nodegroupName?: string;
  nodegroupArn?: string;
  clusterName?: string;
  status?: string;
  capacityType?: string;
  instanceTypes?: string[];
  diskSize?: number;
  scalingConfig?: { desiredSize?: number; minSize?: number; maxSize?: number };
  amiType?: string;
  createdAt?: string | number;
}

interface RawFargateProfile {
  fargateProfileName?: string;
  fargateProfileArn?: string;
  clusterName?: string;
  status?: string;
  podExecutionRoleArn?: string;
  selectors?: { namespace?: string; labels?: Record<string, string> }[];
  createdAt?: string | number;
}

function isoFrom(v?: string | number): string {
  if (v == null || v === "") return "";
  if (typeof v === "number") {
    return new Date(v * (v < 1e12 ? 1000 : 1)).toISOString();
  }
  return v;
}

function mapCluster(raw: RawCluster): Cluster {
  return {
    name: raw.name ?? "",
    arn: raw.arn ?? "",
    status: raw.status ?? "",
    version: raw.version ?? "",
    endpoint: raw.endpoint ?? "",
    roleArn: raw.roleArn ?? "",
    createdAt: isoFrom(raw.createdAt),
  };
}

function mapNodegroup(raw: RawNodegroup): Nodegroup {
  return {
    name: raw.nodegroupName ?? "",
    arn: raw.nodegroupArn ?? "",
    clusterName: raw.clusterName ?? "",
    status: raw.status ?? "",
    capacityType: raw.capacityType ?? "",
    instanceTypes: raw.instanceTypes ?? [],
    diskSize: raw.diskSize ?? 0,
    desiredSize: raw.scalingConfig?.desiredSize ?? 0,
    minSize: raw.scalingConfig?.minSize ?? 0,
    maxSize: raw.scalingConfig?.maxSize ?? 0,
    amiType: raw.amiType ?? "",
    createdAt: isoFrom(raw.createdAt),
  };
}

function mapFargateProfile(raw: RawFargateProfile): FargateProfile {
  return {
    name: raw.fargateProfileName ?? "",
    arn: raw.fargateProfileArn ?? "",
    clusterName: raw.clusterName ?? "",
    status: raw.status ?? "",
    podExecutionRoleArn: raw.podExecutionRoleArn ?? "",
    selectors: (raw.selectors ?? []).map((s) => ({
      namespace: s.namespace ?? "",
      labels: s.labels,
    })),
    createdAt: isoFrom(raw.createdAt),
  };
}

// -- Operations --

export async function listClusters(): Promise<{ clusterNames: string[] }> {
  const data = await eksFetch<{ clusters?: string[] }>(`/clusters`);
  return { clusterNames: data.clusters ?? [] };
}

export async function describeCluster(name: string): Promise<Cluster | null> {
  const data = await eksFetch<{ cluster?: RawCluster }>(
    `/clusters/${encodeURIComponent(name)}`,
  );
  return data.cluster ? mapCluster(data.cluster) : null;
}

export async function listClustersWithDetail(): Promise<{
  clusters: Cluster[];
}> {
  const { clusterNames } = await listClusters();
  const out: Cluster[] = [];
  for (const n of clusterNames) {
    try {
      const c = await describeCluster(n);
      if (c) out.push(c);
    } catch {
      // skip
    }
  }
  return { clusters: out };
}

export interface CreateClusterInput {
  name: string;
  version: string;
  roleArn: string;
  subnetIds?: string[];
}

export async function createCluster(input: CreateClusterInput): Promise<void> {
  await eksFetch(`/clusters`, {
    method: "POST",
    body: JSON.stringify({
      name: input.name,
      version: input.version,
      roleArn: input.roleArn,
      resourcesVpcConfig: {
        subnetIds: input.subnetIds ?? ["subnet-1", "subnet-2"],
      },
    }),
  });
}

export async function deleteCluster(name: string): Promise<void> {
  await eksFetch(`/clusters/${encodeURIComponent(name)}`, {
    method: "DELETE",
  });
}

export async function listNodegroups(
  clusterName: string,
): Promise<{ nodegroupNames: string[] }> {
  const data = await eksFetch<{ nodegroups?: string[] }>(
    `/clusters/${encodeURIComponent(clusterName)}/node-groups`,
  );
  return { nodegroupNames: data.nodegroups ?? [] };
}

export async function describeNodegroup(
  clusterName: string,
  nodegroupName: string,
): Promise<Nodegroup | null> {
  const data = await eksFetch<{ nodegroup?: RawNodegroup }>(
    `/clusters/${encodeURIComponent(clusterName)}/node-groups/${encodeURIComponent(nodegroupName)}`,
  );
  return data.nodegroup ? mapNodegroup(data.nodegroup) : null;
}

export async function listNodegroupsWithDetail(
  clusterName: string,
): Promise<{ nodegroups: Nodegroup[] }> {
  const { nodegroupNames } = await listNodegroups(clusterName);
  const out: Nodegroup[] = [];
  for (const n of nodegroupNames) {
    try {
      const ng = await describeNodegroup(clusterName, n);
      if (ng) out.push(ng);
    } catch {
      // skip
    }
  }
  return { nodegroups: out };
}

export async function listFargateProfiles(
  clusterName: string,
): Promise<{ profileNames: string[] }> {
  const data = await eksFetch<{
    fargateProfileNames?: string[];
    fargateProfiles?: string[];
  }>(`/clusters/${encodeURIComponent(clusterName)}/fargate-profiles`);
  return {
    profileNames: data.fargateProfileNames ?? data.fargateProfiles ?? [],
  };
}

export async function describeFargateProfile(
  clusterName: string,
  profileName: string,
): Promise<FargateProfile | null> {
  const data = await eksFetch<{ fargateProfile?: RawFargateProfile }>(
    `/clusters/${encodeURIComponent(clusterName)}/fargate-profiles/${encodeURIComponent(profileName)}`,
  );
  return data.fargateProfile ? mapFargateProfile(data.fargateProfile) : null;
}

export async function listFargateProfilesWithDetail(
  clusterName: string,
): Promise<{ profiles: FargateProfile[] }> {
  const { profileNames } = await listFargateProfiles(clusterName);
  const out: FargateProfile[] = [];
  for (const n of profileNames) {
    try {
      const p = await describeFargateProfile(clusterName, n);
      if (p) out.push(p);
    } catch {
      // skip
    }
  }
  return { profiles: out };
}
