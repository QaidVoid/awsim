/**
 * Typed DynamoDB API client.
 *
 * Wraps the LocalStack DynamoDB JSON-RPC API behind strongly typed
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
  status: string;
  itemCount: number;
  tableSizeBytes: number;
  keySchema: KeySchemaElement[];
  attributeDefinitions: AttributeDefinition[];
  globalSecondaryIndexes: GlobalSecondaryIndex[];
  localSecondaryIndexes: LocalSecondaryIndex[];
  createdAt: string;
  billingMode: string;
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
  TableStatus?: string;
  ItemCount?: number;
  TableSizeBytes?: number;
  CreationDateTime?: number;
  BillingModeSummary?: { BillingMode?: string };
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
  };
}

export async function listTables(): Promise<TableSummary[]> {
  const data = await request<{ TableNames?: string[] }>("ListTables", {});
  return (data.TableNames ?? []).map((name) => ({ name }));
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
}

export async function createTable(params: CreateTableParams): Promise<void> {
  const attributeDefinitions: { AttributeName: string; AttributeType: string }[] = [
    { AttributeName: params.partitionKey, AttributeType: params.partitionKeyType },
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
  });
}

export async function deleteTable(name: string): Promise<void> {
  await request("DeleteTable", { TableName: name });
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

export async function getItem(tableName: string, key: Item): Promise<Item | null> {
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
