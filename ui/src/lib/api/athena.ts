/**
 * Typed Athena API client.
 *
 * Uses the AWS JSON RPC protocol with `X-Amz-Target: AmazonAthena.<Op>`
 * targeting AWSim endpoints.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "athena";
const TARGET_PREFIX = "AmazonAthena";

// ---------- Types ----------

export interface WorkGroup {
  name: string;
  state: string;
  description: string;
  creationTime: string | null;
}

export interface WorkGroupDetail extends WorkGroup {
  outputLocation: string | null;
  enforceWorkGroupConfiguration: boolean;
  publishCloudWatchMetricsEnabled: boolean;
  bytesScannedCutoffPerQuery: number | null;
}

export interface NamedQuery {
  namedQueryId: string;
  name: string;
  description: string;
  database: string;
  queryString: string;
  workGroup: string;
}

export interface QueryExecutionStatus {
  state: string;
  stateChangeReason: string | null;
  submissionDateTime: string | null;
  completionDateTime: string | null;
}

export interface QueryExecutionStats {
  engineExecutionTimeInMillis: number;
  dataScannedInBytes: number;
}

export interface QueryExecution {
  queryExecutionId: string;
  query: string;
  workGroup: string;
  database: string | null;
  catalog: string | null;
  outputLocation: string | null;
  status: QueryExecutionStatus;
  statistics: QueryExecutionStats | null;
}

export interface QueryResultColumn {
  name: string;
  type: string;
}

export interface QueryResults {
  columns: QueryResultColumn[];
  rows: string[][];
  nextToken: string | null;
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
    throw new Error(`Athena ${action} failed (HTTP ${res.status}): ${text}`);
  }
  const text = await res.text();
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawWorkGroupSummary {
  Name?: string;
  State?: string;
  Description?: string;
  CreationTime?: string;
}

interface RawWorkGroupConfig {
  ResultConfiguration?: { OutputLocation?: string };
  EnforceWorkGroupConfiguration?: boolean;
  PublishCloudWatchMetricsEnabled?: boolean;
  BytesScannedCutoffPerQuery?: number;
}

interface RawWorkGroup extends RawWorkGroupSummary {
  Configuration?: RawWorkGroupConfig;
}

interface RawNamedQuery {
  NamedQueryId?: string;
  Name?: string;
  Description?: string;
  Database?: string;
  QueryString?: string;
  WorkGroup?: string;
}

interface RawQueryExecution {
  QueryExecutionId?: string;
  Query?: string;
  WorkGroup?: string;
  StatementType?: string;
  ResultConfiguration?: { OutputLocation?: string };
  QueryExecutionContext?: { Database?: string; Catalog?: string };
  Status?: {
    State?: string;
    StateChangeReason?: string | null;
    SubmissionDateTime?: string | null;
    CompletionDateTime?: string | null;
  };
  Statistics?: {
    EngineExecutionTimeInMillis?: number;
    DataScannedInBytes?: number;
  };
}

function mapWorkGroup(raw: RawWorkGroupSummary | RawWorkGroup): WorkGroup {
  return {
    name: raw.Name ?? "",
    state: raw.State ?? "",
    description: raw.Description ?? "",
    creationTime: raw.CreationTime ?? null,
  };
}

function mapWorkGroupDetail(raw: RawWorkGroup): WorkGroupDetail {
  const cfg = raw.Configuration ?? {};
  return {
    ...mapWorkGroup(raw),
    outputLocation: cfg.ResultConfiguration?.OutputLocation ?? null,
    enforceWorkGroupConfiguration: cfg.EnforceWorkGroupConfiguration ?? false,
    publishCloudWatchMetricsEnabled:
      cfg.PublishCloudWatchMetricsEnabled ?? false,
    bytesScannedCutoffPerQuery: cfg.BytesScannedCutoffPerQuery ?? null,
  };
}

function mapNamedQuery(raw: RawNamedQuery): NamedQuery {
  return {
    namedQueryId: raw.NamedQueryId ?? "",
    name: raw.Name ?? "",
    description: raw.Description ?? "",
    database: raw.Database ?? "",
    queryString: raw.QueryString ?? "",
    workGroup: raw.WorkGroup ?? "primary",
  };
}

function mapQueryExecution(raw: RawQueryExecution): QueryExecution {
  return {
    queryExecutionId: raw.QueryExecutionId ?? "",
    query: raw.Query ?? "",
    workGroup: raw.WorkGroup ?? "primary",
    database: raw.QueryExecutionContext?.Database ?? null,
    catalog: raw.QueryExecutionContext?.Catalog ?? null,
    outputLocation: raw.ResultConfiguration?.OutputLocation ?? null,
    status: {
      state: raw.Status?.State ?? "",
      stateChangeReason: raw.Status?.StateChangeReason ?? null,
      submissionDateTime: raw.Status?.SubmissionDateTime ?? null,
      completionDateTime: raw.Status?.CompletionDateTime ?? null,
    },
    statistics: raw.Statistics
      ? {
          engineExecutionTimeInMillis:
            raw.Statistics.EngineExecutionTimeInMillis ?? 0,
          dataScannedInBytes: raw.Statistics.DataScannedInBytes ?? 0,
        }
      : null,
  };
}

// ---------- WorkGroups ----------

export async function listWorkGroups(): Promise<WorkGroup[]> {
  const res = await request<{ WorkGroups?: RawWorkGroupSummary[] }>(
    "ListWorkGroups",
  );
  return (res.WorkGroups ?? []).map(mapWorkGroup);
}

export async function getWorkGroup(name: string): Promise<WorkGroupDetail> {
  const res = await request<{ WorkGroup?: RawWorkGroup }>("GetWorkGroup", {
    WorkGroup: name,
  });
  return mapWorkGroupDetail(res.WorkGroup ?? {});
}

// ---------- Named queries ----------

export async function listNamedQueries(workGroup?: string): Promise<string[]> {
  const body: Record<string, unknown> = {};
  if (workGroup) body.WorkGroup = workGroup;
  const res = await request<{ NamedQueryIds?: string[] }>(
    "ListNamedQueries",
    body,
  );
  return res.NamedQueryIds ?? [];
}

export async function getNamedQuery(id: string): Promise<NamedQuery> {
  const res = await request<{ NamedQuery?: RawNamedQuery }>("GetNamedQuery", {
    NamedQueryId: id,
  });
  return mapNamedQuery(res.NamedQuery ?? {});
}

export async function batchGetNamedQuery(ids: string[]): Promise<NamedQuery[]> {
  if (ids.length === 0) return [];
  const res = await request<{ NamedQueries?: RawNamedQuery[] }>(
    "BatchGetNamedQuery",
    { NamedQueryIds: ids },
  );
  return (res.NamedQueries ?? []).map(mapNamedQuery);
}

// ---------- Query executions ----------

export async function listQueryExecutions(
  workGroup?: string,
): Promise<string[]> {
  const body: Record<string, unknown> = {};
  if (workGroup) body.WorkGroup = workGroup;
  const res = await request<{ QueryExecutionIds?: string[] }>(
    "ListQueryExecutions",
    body,
  );
  return res.QueryExecutionIds ?? [];
}

export async function getQueryExecution(id: string): Promise<QueryExecution> {
  const res = await request<{ QueryExecution?: RawQueryExecution }>(
    "GetQueryExecution",
    { QueryExecutionId: id },
  );
  return mapQueryExecution(res.QueryExecution ?? {});
}

export async function batchGetQueryExecution(
  ids: string[],
): Promise<QueryExecution[]> {
  if (ids.length === 0) return [];
  const res = await request<{ QueryExecutions?: RawQueryExecution[] }>(
    "BatchGetQueryExecution",
    { QueryExecutionIds: ids },
  );
  return (res.QueryExecutions ?? []).map(mapQueryExecution);
}

export interface StartQueryInput {
  queryString: string;
  workGroup?: string;
  database?: string;
  outputLocation?: string;
}

export async function startQueryExecution(
  input: StartQueryInput,
): Promise<string> {
  const body: Record<string, unknown> = {
    QueryString: input.queryString,
  };
  if (input.workGroup) body.WorkGroup = input.workGroup;
  if (input.database) {
    body.QueryExecutionContext = { Database: input.database };
  }
  if (input.outputLocation) {
    body.ResultConfiguration = { OutputLocation: input.outputLocation };
  }
  const res = await request<{ QueryExecutionId?: string }>(
    "StartQueryExecution",
    body,
  );
  return res.QueryExecutionId ?? "";
}

export async function stopQueryExecution(id: string): Promise<void> {
  await request<unknown>("StopQueryExecution", { QueryExecutionId: id });
}

interface RawResultColumnInfo {
  Name?: string;
  Type?: string;
}

interface RawResultRow {
  Data?: Array<{ VarCharValue?: string }>;
}

interface RawResultSet {
  ResultSetMetadata?: { ColumnInfo?: RawResultColumnInfo[] };
  Rows?: RawResultRow[];
}

export async function getQueryResults(
  id: string,
  maxResults?: number,
): Promise<QueryResults> {
  const body: Record<string, unknown> = { QueryExecutionId: id };
  if (maxResults) body.MaxResults = maxResults;
  const res = await request<{
    ResultSet?: RawResultSet;
    NextToken?: string | null;
  }>("GetQueryResults", body);
  const set = res.ResultSet ?? {};
  const columns = (set.ResultSetMetadata?.ColumnInfo ?? []).map((c) => ({
    name: c.Name ?? "",
    type: c.Type ?? "",
  }));
  const rows = (set.Rows ?? []).map((r) =>
    (r.Data ?? []).map((d) => d.VarCharValue ?? ""),
  );
  return { columns, rows, nextToken: res.NextToken ?? null };
}
