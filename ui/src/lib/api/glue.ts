/**
 * Typed Glue API client.
 *
 * Glue uses the AWS JSON 1.1 protocol with `X-Amz-Target: AWSGlue.<Op>`.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "glue";
const TARGET_PREFIX = "AWSGlue";

// ---------- Types ----------

export interface GlueDatabase {
  name: string;
  description: string;
  locationUri: string;
  createTime: string | null;
  parameters: Record<string, string>;
}

export interface GlueColumn {
  name: string;
  type: string;
  comment: string | null;
}

export interface GlueTable {
  name: string;
  databaseName: string;
  owner: string;
  tableType: string;
  createTime: string | null;
  updateTime: string | null;
  storageLocation: string | null;
  inputFormat: string | null;
  outputFormat: string | null;
  serdeName: string | null;
  columns: GlueColumn[];
  partitionKeys: GlueColumn[];
  parameters: Record<string, string>;
}

export interface GlueCrawlerTarget {
  type: "s3" | "jdbc" | "dynamodb" | "catalog" | "other";
  path: string;
}

export interface GlueCrawler {
  name: string;
  role: string;
  databaseName: string | null;
  state: string;
  schedule: string | null;
  lastCrawlState: string | null;
  lastCrawlTime: string | null;
  targets: GlueCrawlerTarget[];
  tablePrefix: string | null;
}

export interface GlueJob {
  name: string;
  role: string;
  command: { name: string; scriptLocation: string; pythonVersion: string };
  glueVersion: string | null;
  workerType: string | null;
  numberOfWorkers: number | null;
  timeout: number | null;
  maxRetries: number | null;
  createdOn: string | null;
  lastModifiedOn: string | null;
}

export interface GlueConnection {
  name: string;
  description: string;
  connectionType: string;
  matchCriteria: string[];
  properties: Record<string, string>;
  creationTime: string | null;
  lastUpdatedTime: string | null;
}

// ---------- Internal ----------

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
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Glue ${action} failed (HTTP ${res.status}): ${text}`);
  }
  const text = await res.text();
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawDatabase {
  Name?: string;
  Description?: string;
  LocationUri?: string;
  CreateTime?: string;
  Parameters?: Record<string, string>;
}

interface RawColumn {
  Name?: string;
  Type?: string;
  Comment?: string | null;
}

interface RawStorageDescriptor {
  Location?: string;
  InputFormat?: string;
  OutputFormat?: string;
  SerdeInfo?: { Name?: string };
  Columns?: RawColumn[];
}

interface RawTable {
  Name?: string;
  DatabaseName?: string;
  Owner?: string;
  TableType?: string;
  CreateTime?: string;
  UpdateTime?: string;
  StorageDescriptor?: RawStorageDescriptor;
  PartitionKeys?: RawColumn[];
  Parameters?: Record<string, string>;
}

interface RawCrawlerTargets {
  S3Targets?: Array<{ Path?: string }>;
  JdbcTargets?: Array<{ ConnectionName?: string; Path?: string }>;
  DynamoDBTargets?: Array<{ Path?: string }>;
  CatalogTargets?: Array<{ DatabaseName?: string; Tables?: string[] }>;
}

interface RawCrawler {
  Name?: string;
  Role?: string;
  DatabaseName?: string;
  State?: string;
  Schedule?: { ScheduleExpression?: string };
  LastCrawl?: { Status?: string; StartTime?: string };
  Targets?: RawCrawlerTargets;
  TablePrefix?: string;
}

interface RawJob {
  Name?: string;
  Role?: string;
  Command?: {
    Name?: string;
    ScriptLocation?: string;
    PythonVersion?: string;
  };
  GlueVersion?: string;
  WorkerType?: string;
  NumberOfWorkers?: number;
  Timeout?: number;
  MaxRetries?: number;
  CreatedOn?: string;
  LastModifiedOn?: string;
}

interface RawConnection {
  Name?: string;
  Description?: string;
  ConnectionType?: string;
  MatchCriteria?: string[];
  ConnectionProperties?: Record<string, string>;
  CreationTime?: string;
  LastUpdatedTime?: string;
}

function mapDatabase(raw: RawDatabase): GlueDatabase {
  return {
    name: raw.Name ?? "",
    description: raw.Description ?? "",
    locationUri: raw.LocationUri ?? "",
    createTime: raw.CreateTime ?? null,
    parameters: raw.Parameters ?? {},
  };
}

function mapColumn(raw: RawColumn): GlueColumn {
  return {
    name: raw.Name ?? "",
    type: raw.Type ?? "",
    comment: raw.Comment ?? null,
  };
}

function mapTable(raw: RawTable): GlueTable {
  const sd = raw.StorageDescriptor ?? {};
  return {
    name: raw.Name ?? "",
    databaseName: raw.DatabaseName ?? "",
    owner: raw.Owner ?? "",
    tableType: raw.TableType ?? "",
    createTime: raw.CreateTime ?? null,
    updateTime: raw.UpdateTime ?? null,
    storageLocation: sd.Location ?? null,
    inputFormat: sd.InputFormat ?? null,
    outputFormat: sd.OutputFormat ?? null,
    serdeName: sd.SerdeInfo?.Name ?? null,
    columns: (sd.Columns ?? []).map(mapColumn),
    partitionKeys: (raw.PartitionKeys ?? []).map(mapColumn),
    parameters: raw.Parameters ?? {},
  };
}

function flattenTargets(t: RawCrawlerTargets | undefined): GlueCrawlerTarget[] {
  const out: GlueCrawlerTarget[] = [];
  for (const s of t?.S3Targets ?? []) {
    if (s.Path) out.push({ type: "s3", path: s.Path });
  }
  for (const j of t?.JdbcTargets ?? []) {
    if (j.Path) out.push({ type: "jdbc", path: j.Path });
  }
  for (const d of t?.DynamoDBTargets ?? []) {
    if (d.Path) out.push({ type: "dynamodb", path: d.Path });
  }
  for (const c of t?.CatalogTargets ?? []) {
    if (c.DatabaseName) {
      out.push({
        type: "catalog",
        path: `${c.DatabaseName}.${(c.Tables ?? []).join(",")}`,
      });
    }
  }
  return out;
}

function mapCrawler(raw: RawCrawler): GlueCrawler {
  return {
    name: raw.Name ?? "",
    role: raw.Role ?? "",
    databaseName: raw.DatabaseName ?? null,
    state: raw.State ?? "",
    schedule: raw.Schedule?.ScheduleExpression ?? null,
    lastCrawlState: raw.LastCrawl?.Status ?? null,
    lastCrawlTime: raw.LastCrawl?.StartTime ?? null,
    targets: flattenTargets(raw.Targets),
    tablePrefix: raw.TablePrefix ?? null,
  };
}

function mapJob(raw: RawJob): GlueJob {
  return {
    name: raw.Name ?? "",
    role: raw.Role ?? "",
    command: {
      name: raw.Command?.Name ?? "",
      scriptLocation: raw.Command?.ScriptLocation ?? "",
      pythonVersion: raw.Command?.PythonVersion ?? "",
    },
    glueVersion: raw.GlueVersion ?? null,
    workerType: raw.WorkerType ?? null,
    numberOfWorkers: raw.NumberOfWorkers ?? null,
    timeout: raw.Timeout ?? null,
    maxRetries: raw.MaxRetries ?? null,
    createdOn: raw.CreatedOn ?? null,
    lastModifiedOn: raw.LastModifiedOn ?? null,
  };
}

function mapConnection(raw: RawConnection): GlueConnection {
  return {
    name: raw.Name ?? "",
    description: raw.Description ?? "",
    connectionType: raw.ConnectionType ?? "",
    matchCriteria: raw.MatchCriteria ?? [],
    properties: raw.ConnectionProperties ?? {},
    creationTime: raw.CreationTime ?? null,
    lastUpdatedTime: raw.LastUpdatedTime ?? null,
  };
}

// ---------- Operations ----------

export async function getDatabases(): Promise<GlueDatabase[]> {
  const res = await request<{ DatabaseList?: RawDatabase[] }>("GetDatabases");
  return (res.DatabaseList ?? []).map(mapDatabase);
}

export async function getDatabase(name: string): Promise<GlueDatabase> {
  const res = await request<{ Database?: RawDatabase }>("GetDatabase", {
    Name: name,
  });
  return mapDatabase(res.Database ?? {});
}

export async function getTables(databaseName: string): Promise<GlueTable[]> {
  const res = await request<{ TableList?: RawTable[] }>("GetTables", {
    DatabaseName: databaseName,
  });
  return (res.TableList ?? []).map(mapTable);
}

export async function getTable(
  databaseName: string,
  name: string,
): Promise<GlueTable> {
  const res = await request<{ Table?: RawTable }>("GetTable", {
    DatabaseName: databaseName,
    Name: name,
  });
  return mapTable(res.Table ?? {});
}

export async function getCrawlers(): Promise<GlueCrawler[]> {
  const res = await request<{ Crawlers?: RawCrawler[] }>("GetCrawlers");
  return (res.Crawlers ?? []).map(mapCrawler);
}

export async function getCrawler(name: string): Promise<GlueCrawler> {
  const res = await request<{ Crawler?: RawCrawler }>("GetCrawler", {
    Name: name,
  });
  return mapCrawler(res.Crawler ?? {});
}

export async function getJobs(): Promise<GlueJob[]> {
  const res = await request<{ Jobs?: RawJob[] }>("GetJobs");
  return (res.Jobs ?? []).map(mapJob);
}

export async function getJob(name: string): Promise<GlueJob> {
  const res = await request<{ Job?: RawJob }>("GetJob", { JobName: name });
  return mapJob(res.Job ?? {});
}

export async function getConnections(): Promise<GlueConnection[]> {
  const res = await request<{ ConnectionList?: RawConnection[] }>(
    "GetConnections",
  );
  return (res.ConnectionList ?? []).map(mapConnection);
}

export async function getConnection(name: string): Promise<GlueConnection> {
  const res = await request<{ Connection?: RawConnection }>("GetConnection", {
    Name: name,
  });
  return mapConnection(res.Connection ?? {});
}
