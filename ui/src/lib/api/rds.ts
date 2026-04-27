/**
 * Typed RDS API client.
 *
 * Wraps LocalStack's RDS XML query protocol with strongly typed
 * helpers for instances, snapshots, and clusters.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const VERSION = "2014-10-31";

export interface DBInstance {
  identifier: string;
  engine: string;
  engineVersion: string;
  status: string;
  endpoint: string;
  port: string;
  instanceClass: string;
  allocatedStorage: number;
  storageType: string;
  masterUsername: string;
  publiclyAccessible: boolean;
  multiAZ: boolean;
  createdAt: string;
  arn: string;
}

export interface DBSnapshot {
  identifier: string;
  dbIdentifier: string;
  engine: string;
  status: string;
  snapshotType: string;
  createdAt: string;
  allocatedStorage: number;
}

export interface DBCluster {
  identifier: string;
  engine: string;
  status: string;
  endpoint: string;
}

async function rdsRequest(
  action: string,
  params: Record<string, string> = {},
): Promise<string> {
  const body = new URLSearchParams({
    Action: action,
    Version: VERSION,
    ...params,
  });
  const res = await loggedFetch("rds", action, "POST", ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-www-form-urlencoded",
      Authorization: authHeader("rds"),
      "X-Amz-Date": amzDate(),
    },
    body: body.toString(),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${action} failed: HTTP ${res.status}: ${text}`);
  return text;
}

function xmlText(xml: string, tag: string): string {
  const m = new RegExp(`<${tag}>([^<]*)</${tag}>`).exec(xml);
  return m ? m[1] : "";
}

function xmlBlocks(xml: string, tag: string): string[] {
  const out: string[] = [];
  const regex = new RegExp(`<${tag}>([\\s\\S]*?)</${tag}>`, "g");
  let m: RegExpExecArray | null;
  while ((m = regex.exec(xml)) !== null) out.push(m[1]);
  return out;
}

function parseInstance(block: string): DBInstance {
  const port =
    xmlText(block, "Port") ||
    (() => {
      const ep = new RegExp(`<Endpoint>([\\s\\S]*?)<\\/Endpoint>`).exec(block);
      return ep ? xmlText(ep[1], "Port") : "";
    })();
  const address = (() => {
    const ep = new RegExp(`<Endpoint>([\\s\\S]*?)<\\/Endpoint>`).exec(block);
    return ep ? xmlText(ep[1], "Address") : "";
  })();
  return {
    identifier: xmlText(block, "DBInstanceIdentifier"),
    engine: xmlText(block, "Engine"),
    engineVersion: xmlText(block, "EngineVersion"),
    status: xmlText(block, "DBInstanceStatus"),
    endpoint: address,
    port,
    instanceClass: xmlText(block, "DBInstanceClass"),
    allocatedStorage: parseInt(xmlText(block, "AllocatedStorage") || "0", 10),
    storageType: xmlText(block, "StorageType"),
    masterUsername: xmlText(block, "MasterUsername"),
    publiclyAccessible: xmlText(block, "PubliclyAccessible") === "true",
    multiAZ: xmlText(block, "MultiAZ") === "true",
    createdAt: xmlText(block, "InstanceCreateTime"),
    arn: xmlText(block, "DBInstanceArn"),
  };
}

function parseSnapshot(block: string): DBSnapshot {
  return {
    identifier: xmlText(block, "DBSnapshotIdentifier"),
    dbIdentifier: xmlText(block, "DBInstanceIdentifier"),
    engine: xmlText(block, "Engine"),
    status: xmlText(block, "Status"),
    snapshotType: xmlText(block, "SnapshotType"),
    createdAt: xmlText(block, "SnapshotCreateTime"),
    allocatedStorage: parseInt(xmlText(block, "AllocatedStorage") || "0", 10),
  };
}

function parseCluster(block: string): DBCluster {
  return {
    identifier: xmlText(block, "DBClusterIdentifier"),
    engine: xmlText(block, "Engine"),
    status: xmlText(block, "Status"),
    endpoint: xmlText(block, "Endpoint"),
  };
}

export async function describeDBInstances(): Promise<DBInstance[]> {
  const xml = await rdsRequest("DescribeDBInstances");
  return xmlBlocks(xml, "DBInstance").map(parseInstance);
}

export interface CreateDBInstanceParams {
  identifier: string;
  engine: string;
  instanceClass: string;
  allocatedStorage: number;
  masterUsername: string;
  masterUserPassword: string;
  publiclyAccessible?: boolean;
}

export async function createDBInstance(
  params: CreateDBInstanceParams,
): Promise<void> {
  await rdsRequest("CreateDBInstance", {
    DBInstanceIdentifier: params.identifier,
    Engine: params.engine,
    DBInstanceClass: params.instanceClass,
    AllocatedStorage: String(params.allocatedStorage),
    MasterUsername: params.masterUsername,
    MasterUserPassword: params.masterUserPassword,
    PubliclyAccessible: params.publiclyAccessible ? "true" : "false",
  });
}

export async function deleteDBInstance(identifier: string): Promise<void> {
  await rdsRequest("DeleteDBInstance", {
    DBInstanceIdentifier: identifier,
    SkipFinalSnapshot: "true",
  });
}

export async function describeDBSnapshots(
  dbIdentifier?: string,
): Promise<DBSnapshot[]> {
  const params: Record<string, string> = {};
  if (dbIdentifier) params.DBInstanceIdentifier = dbIdentifier;
  const xml = await rdsRequest("DescribeDBSnapshots", params);
  return xmlBlocks(xml, "DBSnapshot").map(parseSnapshot);
}

export async function createDBSnapshot(
  dbIdentifier: string,
  snapshotIdentifier: string,
): Promise<void> {
  await rdsRequest("CreateDBSnapshot", {
    DBInstanceIdentifier: dbIdentifier,
    DBSnapshotIdentifier: snapshotIdentifier,
  });
}

export async function deleteDBSnapshot(
  snapshotIdentifier: string,
): Promise<void> {
  await rdsRequest("DeleteDBSnapshot", {
    DBSnapshotIdentifier: snapshotIdentifier,
  });
}

export async function describeDBClusters(): Promise<DBCluster[]> {
  const xml = await rdsRequest("DescribeDBClusters");
  return xmlBlocks(xml, "DBCluster").map(parseCluster);
}

export function statusVariant(
  status: string,
): "default" | "secondary" | "destructive" | "outline" {
  const s = status.toLowerCase();
  if (s === "available") return "secondary";
  if (
    s === "creating" ||
    s === "modifying" ||
    s === "starting" ||
    s === "backing-up"
  )
    return "outline";
  if (s === "deleting" || s === "failed" || s === "stopped")
    return "destructive";
  return "outline";
}

export function formatTimestamp(iso: string): string {
  if (!iso) return "—";
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}
