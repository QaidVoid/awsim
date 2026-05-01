/**
 * Typed DynamoDB API client.
 *
 * Wraps the AWSim DynamoDB JSON-RPC API behind strongly typed
 * helpers. Item attribute values use AWS' attribute-value tagged
 * encoding, exposed here as a discriminated union.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const TARGET_PREFIX = "DynamoDB_20120810";

export type ScalarType = "S" | "N" | "B";

export interface KeySchemaElement {
  attributeName: string;
  keyType: "HASH" | "RANGE";
}

export interface AttributeDefinition {
  attributeName: string;
  attributeType: ScalarType;
}

export interface GlobalSecondaryIndex {
  indexName: string;
  keySchema: KeySchemaElement[];
  projectionType: string;
  itemCount: number;
  indexSizeBytes: number;
  status: string;
}

export interface LocalSecondaryIndex {
  indexName: string;
  keySchema: KeySchemaElement[];
  projectionType: string;
}

export interface TableSummary {
  name: string;
  status?: string;
  itemCount?: number;
  createdAt?: string;
  keySchema?: KeySchemaElement[];
}

export interface TableDetail {
  name: string;
  arn: string;
  status: string;
  itemCount: number;
  tableSizeBytes: number;
  keySchema: KeySchemaElement[];
  attributeDefinitions: AttributeDefinition[];
  globalSecondaryIndexes: GlobalSecondaryIndex[];
  localSecondaryIndexes: LocalSecondaryIndex[];
  createdAt: string;
  billingMode: string;
  deletionProtectionEnabled: boolean;
}

export interface TtlState {
  enabled: boolean;
  attributeName: string;
}

export interface ResourceTag {
  key: string;
  value: string;
}

export type AttributeValue =
  | { S: string }
  | { N: string }
  | { BOOL: boolean }
  | { NULL: true }
  | { L: AttributeValue[] }
  | { M: Record<string, AttributeValue> }
  | { B: string }
  | { SS: string[] }
  | { NS: string[] }
  | { BS: string[] };

export type Item = Record<string, AttributeValue>;

export interface ScanResult {
  items: Item[];
  count: number;
  scannedCount: number;
  lastEvaluatedKey?: Item;
}

export interface QueryParams {
  tableName: string;
  partitionKey: string;
  partitionValue: AttributeValue;
  sortKey?: string;
  sortValue?: AttributeValue;
  sortOperator?: "EQ" | "LT" | "LE" | "GT" | "GE" | "BEGINS_WITH";
  indexName?: string;
  limit?: number;
}

export interface PartiQLResult {
  items: Item[];
  nextToken?: string;
}

async function request<T>(action: string, body: unknown): Promise<T> {
  const res = await loggedFetch("dynamodb", action, "POST", ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.0",
      "X-Amz-Target": `${TARGET_PREFIX}.${action}`,
      Authorization: authHeader("dynamodb"),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`${action} failed: HTTP ${res.status}: ${text}`);
  }
  return (await res.json()) as T;
}

interface RawTableDescription {
  TableName?: string;
  TableArn?: string;
  TableStatus?: string;
  ItemCount?: number;
  TableSizeBytes?: number;
  CreationDateTime?: number;
  BillingModeSummary?: { BillingMode?: string };
  DeletionProtectionEnabled?: boolean;
  KeySchema?: { AttributeName: string; KeyType: string }[];
  AttributeDefinitions?: { AttributeName: string; AttributeType: string }[];
  GlobalSecondaryIndexes?: {
    IndexName: string;
    KeySchema: { AttributeName: string; KeyType: string }[];
    Projection?: { ProjectionType?: string };
    ItemCount?: number;
    IndexSizeBytes?: number;
    IndexStatus?: string;
  }[];
  LocalSecondaryIndexes?: {
    IndexName: string;
    KeySchema: { AttributeName: string; KeyType: string }[];
    Projection?: { ProjectionType?: string };
  }[];
}

function mapKeySchema(
  raw: { AttributeName: string; KeyType: string }[] | undefined,
): KeySchemaElement[] {
  return (raw ?? []).map((k) => ({
    attributeName: k.AttributeName,
    keyType: k.KeyType as "HASH" | "RANGE",
  }));
}

function mapTable(raw: RawTableDescription, fallbackName = ""): TableDetail {
  return {
    name: raw.TableName ?? fallbackName,
    arn: raw.TableArn ?? "",
    status: raw.TableStatus ?? "",
    itemCount: raw.ItemCount ?? 0,
    tableSizeBytes: raw.TableSizeBytes ?? 0,
    keySchema: mapKeySchema(raw.KeySchema),
    attributeDefinitions: (raw.AttributeDefinitions ?? []).map((a) => ({
      attributeName: a.AttributeName,
      attributeType: a.AttributeType as ScalarType,
    })),
    globalSecondaryIndexes: (raw.GlobalSecondaryIndexes ?? []).map((g) => ({
      indexName: g.IndexName,
      keySchema: mapKeySchema(g.KeySchema),
      projectionType: g.Projection?.ProjectionType ?? "",
      itemCount: g.ItemCount ?? 0,
      indexSizeBytes: g.IndexSizeBytes ?? 0,
      status: g.IndexStatus ?? "",
    })),
    localSecondaryIndexes: (raw.LocalSecondaryIndexes ?? []).map((l) => ({
      indexName: l.IndexName,
      keySchema: mapKeySchema(l.KeySchema),
      projectionType: l.Projection?.ProjectionType ?? "",
    })),
    createdAt: raw.CreationDateTime
      ? new Date(raw.CreationDateTime * 1000).toISOString()
      : "",
    billingMode: raw.BillingModeSummary?.BillingMode ?? "PAY_PER_REQUEST",
    deletionProtectionEnabled: raw.DeletionProtectionEnabled ?? false,
  };
}

export async function listTables(): Promise<TableSummary[]> {
  // ListTables caps at 100 names per response and returns
  // `LastEvaluatedTableName` to continue. Walk the cursor until it's
  // gone so the caller sees the full list. AWS's per-account-per-
  // region table cap is 2500 so this is bounded in practice; the
  // loop terminates naturally when the cursor is omitted, and a
  // monotonic cursor check guards against a stuck server.
  const out: TableSummary[] = [];
  let cursor: string | undefined;
  while (true) {
    const body: Record<string, unknown> = { Limit: 100 };
    if (cursor) body.ExclusiveStartTableName = cursor;
    const data = await request<{
      TableNames?: string[];
      LastEvaluatedTableName?: string;
    }>("ListTables", body);
    for (const name of data.TableNames ?? []) {
      out.push({ name });
    }
    const next = data.LastEvaluatedTableName;
    if (!next || next === cursor) break;
    cursor = next;
  }
  return out;
}

export async function describeTable(name: string): Promise<TableDetail> {
  const data = await request<{ Table?: RawTableDescription }>("DescribeTable", {
    TableName: name,
  });
  return mapTable(data.Table ?? {}, name);
}

export interface CreateTableParams {
  name: string;
  partitionKey: string;
  partitionKeyType: ScalarType;
  sortKey?: string;
  sortKeyType?: ScalarType;
  deletionProtectionEnabled?: boolean;
}

export async function createTable(params: CreateTableParams): Promise<void> {
  const attributeDefinitions: {
    AttributeName: string;
    AttributeType: string;
  }[] = [
    {
      AttributeName: params.partitionKey,
      AttributeType: params.partitionKeyType,
    },
  ];
  const keySchema: { AttributeName: string; KeyType: string }[] = [
    { AttributeName: params.partitionKey, KeyType: "HASH" },
  ];
  if (params.sortKey) {
    attributeDefinitions.push({
      AttributeName: params.sortKey,
      AttributeType: params.sortKeyType ?? "S",
    });
    keySchema.push({ AttributeName: params.sortKey, KeyType: "RANGE" });
  }
  await request("CreateTable", {
    TableName: params.name,
    AttributeDefinitions: attributeDefinitions,
    KeySchema: keySchema,
    BillingMode: "PAY_PER_REQUEST",
    DeletionProtectionEnabled: params.deletionProtectionEnabled ?? false,
  });
}

export async function deleteTable(name: string): Promise<void> {
  await request("DeleteTable", { TableName: name });
}

/**
 * Toggle DeletionProtectionEnabled. AWS rejects DeleteTable when this
 * is true; flip it off via this call before retrying a delete.
 */
export async function setDeletionProtection(
  name: string,
  enabled: boolean,
): Promise<void> {
  await request("UpdateTable", {
    TableName: name,
    DeletionProtectionEnabled: enabled,
  });
}

export async function setBillingMode(
  name: string,
  mode: "PAY_PER_REQUEST" | "PROVISIONED",
): Promise<void> {
  await request("UpdateTable", {
    TableName: name,
    BillingMode: mode,
  });
}

export async function describeTtl(name: string): Promise<TtlState> {
  const data = await request<{
    TimeToLiveDescription?: {
      TimeToLiveStatus?: string;
      AttributeName?: string;
    };
  }>("DescribeTimeToLive", { TableName: name });
  const desc = data.TimeToLiveDescription ?? {};
  return {
    enabled: desc.TimeToLiveStatus === "ENABLED",
    attributeName: desc.AttributeName ?? "",
  };
}

export async function updateTtl(
  name: string,
  enabled: boolean,
  attributeName: string,
): Promise<void> {
  await request("UpdateTimeToLive", {
    TableName: name,
    TimeToLiveSpecification: {
      Enabled: enabled,
      AttributeName: attributeName,
    },
  });
}

export async function listTags(arn: string): Promise<ResourceTag[]> {
  const data = await request<{ Tags?: { Key: string; Value: string }[] }>(
    "ListTagsOfResource",
    { ResourceArn: arn },
  );
  return (data.Tags ?? []).map((t) => ({ key: t.Key, value: t.Value }));
}

export async function tagResource(
  arn: string,
  tags: ResourceTag[],
): Promise<void> {
  await request("TagResource", {
    ResourceArn: arn,
    Tags: tags.map((t) => ({ Key: t.key, Value: t.value })),
  });
}

export async function untagResource(
  arn: string,
  keys: string[],
): Promise<void> {
  await request("UntagResource", {
    ResourceArn: arn,
    TagKeys: keys,
  });
}

/**
 * awsim-only operation — clears every item in a table without dropping
 * the schema, GSIs, or stream config. Returns how many items were
 * removed. Real DynamoDB doesn't support this; awsim keeps it for the
 * "wipe and retest" loop the UI runs against local data.
 */
export async function truncateTable(name: string): Promise<number> {
  const data = await request<{ DeletedItemCount?: number }>("TruncateTable", {
    TableName: name,
  });
  return data.DeletedItemCount ?? 0;
}

export interface ScanParams {
  tableName: string;
  limit?: number;
  exclusiveStartKey?: Item;
  indexName?: string;
}

export async function scan(params: ScanParams): Promise<ScanResult> {
  const data = await request<{
    Items?: Item[];
    Count?: number;
    ScannedCount?: number;
    LastEvaluatedKey?: Item;
  }>("Scan", {
    TableName: params.tableName,
    Limit: params.limit ?? 50,
    ...(params.exclusiveStartKey
      ? { ExclusiveStartKey: params.exclusiveStartKey }
      : {}),
    ...(params.indexName ? { IndexName: params.indexName } : {}),
  });
  return {
    items: data.Items ?? [],
    count: data.Count ?? 0,
    scannedCount: data.ScannedCount ?? 0,
    lastEvaluatedKey: data.LastEvaluatedKey,
  };
}

const OPERATOR_TO_EXPR: Record<string, string> = {
  EQ: "=",
  LT: "<",
  LE: "<=",
  GT: ">",
  GE: ">=",
};

export async function query(params: QueryParams): Promise<ScanResult> {
  const expressionAttributeNames: Record<string, string> = {
    "#pk": params.partitionKey,
  };
  const expressionAttributeValues: Record<string, AttributeValue> = {
    ":pk": params.partitionValue,
  };
  let keyConditionExpression = "#pk = :pk";

  if (params.sortKey && params.sortValue !== undefined) {
    expressionAttributeNames["#sk"] = params.sortKey;
    expressionAttributeValues[":sk"] = params.sortValue;
    if (params.sortOperator === "BEGINS_WITH") {
      keyConditionExpression += " AND begins_with(#sk, :sk)";
    } else {
      const op = OPERATOR_TO_EXPR[params.sortOperator ?? "EQ"];
      keyConditionExpression += ` AND #sk ${op} :sk`;
    }
  }

  const data = await request<{
    Items?: Item[];
    Count?: number;
    ScannedCount?: number;
    LastEvaluatedKey?: Item;
  }>("Query", {
    TableName: params.tableName,
    KeyConditionExpression: keyConditionExpression,
    ExpressionAttributeNames: expressionAttributeNames,
    ExpressionAttributeValues: expressionAttributeValues,
    ...(params.indexName ? { IndexName: params.indexName } : {}),
    ...(params.limit ? { Limit: params.limit } : {}),
  });
  return {
    items: data.Items ?? [],
    count: data.Count ?? 0,
    scannedCount: data.ScannedCount ?? 0,
    lastEvaluatedKey: data.LastEvaluatedKey,
  };
}

export async function getItem(
  tableName: string,
  key: Item,
): Promise<Item | null> {
  const data = await request<{ Item?: Item }>("GetItem", {
    TableName: tableName,
    Key: key,
  });
  return data.Item ?? null;
}

export async function putItem(tableName: string, item: Item): Promise<void> {
  await request("PutItem", { TableName: tableName, Item: item });
}

export async function deleteItem(tableName: string, key: Item): Promise<void> {
  await request("DeleteItem", { TableName: tableName, Key: key });
}

export async function executeStatement(
  statement: string,
  parameters?: AttributeValue[],
  nextToken?: string,
): Promise<PartiQLResult> {
  const body: Record<string, unknown> = { Statement: statement };
  if (parameters && parameters.length > 0) body.Parameters = parameters;
  if (nextToken) body.NextToken = nextToken;
  const data = await request<{ Items?: Item[]; NextToken?: string }>(
    "ExecuteStatement",
    body,
  );
  return { items: data.Items ?? [], nextToken: data.NextToken };
}

// ---- Helpers for working with AttributeValue ----

export function attributeType(v: AttributeValue): string {
  return Object.keys(v)[0] ?? "?";
}

export function attributeToString(v: AttributeValue): string {
  if ("S" in v) return v.S;
  if ("N" in v) return v.N;
  if ("BOOL" in v) return String(v.BOOL);
  if ("NULL" in v) return "null";
  if ("L" in v) return `[${v.L.map(attributeToString).join(", ")}]`;
  if ("M" in v) {
    return `{${Object.entries(v.M)
      .map(([k, val]) => `${k}: ${attributeToString(val)}`)
      .join(", ")}}`;
  }
  if ("SS" in v) return `{${v.SS.join(", ")}}`;
  if ("NS" in v) return `{${v.NS.join(", ")}}`;
  if ("BS" in v) return `{${v.BS.join(", ")}}`;
  if ("B" in v) return v.B;
  return JSON.stringify(v);
}

export function inferAttribute(
  raw: string,
  type: ScalarType | "BOOL" | "NULL",
): AttributeValue {
  switch (type) {
    case "S":
      return { S: raw };
    case "N":
      return { N: raw };
    case "B":
      return { B: raw };
    case "BOOL":
      return { BOOL: raw === "true" };
    case "NULL":
      return { NULL: true };
  }
}

// -- Global Tables --

export interface GlobalTableReplica {
  regionName: string;
  replicaStatus?: string;
}

export interface GlobalTable {
  globalTableName: string;
  globalTableArn?: string;
  globalTableStatus?: string;
  creationDateTime?: number;
  replicationGroup: GlobalTableReplica[];
}

export async function listGlobalTables(): Promise<GlobalTable[]> {
  const resp = (await request("ListGlobalTables", {})) as {
    GlobalTables?: Array<{
      GlobalTableName: string;
      ReplicationGroup?: Array<{ RegionName: string }>;
    }>;
  };
  return (resp.GlobalTables ?? []).map((g) => ({
    globalTableName: g.GlobalTableName,
    replicationGroup: (g.ReplicationGroup ?? []).map((r) => ({
      regionName: r.RegionName,
    })),
  }));
}

export async function describeGlobalTable(name: string): Promise<GlobalTable> {
  const resp = (await request("DescribeGlobalTable", {
    GlobalTableName: name,
  })) as {
    GlobalTableDescription: {
      GlobalTableName: string;
      GlobalTableArn?: string;
      GlobalTableStatus?: string;
      CreationDateTime?: number;
      ReplicationGroup?: Array<{ RegionName: string; ReplicaStatus?: string }>;
    };
  };
  const g = resp.GlobalTableDescription;
  return {
    globalTableName: g.GlobalTableName,
    globalTableArn: g.GlobalTableArn,
    globalTableStatus: g.GlobalTableStatus,
    creationDateTime: g.CreationDateTime,
    replicationGroup: (g.ReplicationGroup ?? []).map((r) => ({
      regionName: r.RegionName,
      replicaStatus: r.ReplicaStatus,
    })),
  };
}

export async function createGlobalTable(
  name: string,
  regions: string[],
): Promise<GlobalTable> {
  const resp = (await request("CreateGlobalTable", {
    GlobalTableName: name,
    ReplicationGroup: regions.map((r) => ({ RegionName: r })),
  })) as {
    GlobalTableDescription: {
      GlobalTableName: string;
      GlobalTableArn?: string;
      GlobalTableStatus?: string;
      CreationDateTime?: number;
      ReplicationGroup?: Array<{ RegionName: string; ReplicaStatus?: string }>;
    };
  };
  const g = resp.GlobalTableDescription;
  return {
    globalTableName: g.GlobalTableName,
    globalTableArn: g.GlobalTableArn,
    globalTableStatus: g.GlobalTableStatus,
    creationDateTime: g.CreationDateTime,
    replicationGroup: (g.ReplicationGroup ?? []).map((r) => ({
      regionName: r.RegionName,
      replicaStatus: r.ReplicaStatus,
    })),
  };
}

export interface ReplicaUpdate {
  create?: string;
  delete?: string;
}

export async function updateGlobalTable(
  name: string,
  updates: ReplicaUpdate[],
): Promise<GlobalTable> {
  const resp = (await request("UpdateGlobalTable", {
    GlobalTableName: name,
    ReplicaUpdates: updates.map((u) => {
      if (u.create) return { Create: { RegionName: u.create } };
      if (u.delete) return { Delete: { RegionName: u.delete } };
      return {};
    }),
  })) as {
    GlobalTableDescription: {
      GlobalTableName: string;
      GlobalTableArn?: string;
      GlobalTableStatus?: string;
      CreationDateTime?: number;
      ReplicationGroup?: Array<{ RegionName: string; ReplicaStatus?: string }>;
    };
  };
  const g = resp.GlobalTableDescription;
  return {
    globalTableName: g.GlobalTableName,
    globalTableArn: g.GlobalTableArn,
    globalTableStatus: g.GlobalTableStatus,
    creationDateTime: g.CreationDateTime,
    replicationGroup: (g.ReplicationGroup ?? []).map((r) => ({
      regionName: r.RegionName,
      replicaStatus: r.ReplicaStatus,
    })),
  };
}
