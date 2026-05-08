/**
 * CloudWatch Logs API client.
 *
 * Thin typed wrappers over the AWSim Logs JSON 1.1
 * protocol (`X-Amz-Target: Logs_20140328.<Action>`). Every operation is
 * normalised into camel-cased, `undefined`-safe shapes so the UI never
 * has to reason about the AWS wire format.
 */

const ENDPOINT = "http://localhost:4566";
const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, "");

function authHeader(service: string): string {
  return `AWS4-HMAC-SHA256 Credential=awsim-admin/${FAKE_DATE}/us-east-1/${service}/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

function amzDate(): string {
  return new Date().toISOString().replace(/[:-]/g, "").slice(0, 15) + "Z";
}

async function logsRequest<T>(
  action: string,
  body: Record<string, unknown>,
): Promise<T> {
  const res = await fetch(`${ENDPOINT}/`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `Logs_20140328.${action}`,
      Authorization: authHeader("logs"),
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

export interface LogGroup {
  name: string;
  arn?: string;
  retentionDays?: number;
  storedBytes: number;
  creationTime?: number;
}

export interface LogStream {
  name: string;
  arn?: string;
  storedBytes: number;
  firstEventTimestamp?: number;
  lastEventTimestamp?: number;
  creationTime?: number;
}

export interface LogEvent {
  timestamp: number;
  message: string;
  ingestionTime?: number;
  eventId?: string;
}

export interface FilteredLogEvent extends LogEvent {
  logStreamName: string;
}

export interface InsightsQueryResultRow {
  field: string;
  value: string;
}

export interface InsightsQueryStatus {
  queryId: string;
  status: string;
  results: InsightsQueryResultRow[][];
  statistics?: {
    recordsMatched?: number;
    recordsScanned?: number;
    bytesScanned?: number;
  };
}

// -- Raw response shapes --

interface RawLogGroup {
  logGroupName?: string;
  arn?: string;
  retentionInDays?: number;
  storedBytes?: number;
  creationTime?: number;
}

interface RawLogStream {
  logStreamName?: string;
  arn?: string;
  storedBytes?: number;
  firstEventTimestamp?: number;
  lastEventTimestamp?: number;
  creationTime?: number;
}

interface RawLogEvent {
  timestamp?: number;
  message?: string;
  ingestionTime?: number;
  eventId?: string;
  logStreamName?: string;
}

// -- Operations --

export async function describeLogGroups(): Promise<{ logGroups: LogGroup[] }> {
  const data = await logsRequest<{ logGroups?: RawLogGroup[] }>(
    "DescribeLogGroups",
    {},
  );
  return {
    logGroups: (data.logGroups ?? []).map((g) => ({
      name: g.logGroupName ?? "",
      arn: g.arn,
      retentionDays: g.retentionInDays,
      storedBytes: g.storedBytes ?? 0,
      creationTime: g.creationTime,
    })),
  };
}

export async function createLogGroup(name: string): Promise<void> {
  await logsRequest("CreateLogGroup", { logGroupName: name });
}

export async function deleteLogGroup(name: string): Promise<void> {
  await logsRequest("DeleteLogGroup", { logGroupName: name });
}

export async function describeLogStreams(
  logGroupName: string,
): Promise<{ logStreams: LogStream[] }> {
  const data = await logsRequest<{ logStreams?: RawLogStream[] }>(
    "DescribeLogStreams",
    {
      logGroupName,
      orderBy: "LastEventTime",
      descending: true,
      limit: 50,
    },
  );
  return {
    logStreams: (data.logStreams ?? []).map((s) => ({
      name: s.logStreamName ?? "",
      arn: s.arn,
      storedBytes: s.storedBytes ?? 0,
      firstEventTimestamp: s.firstEventTimestamp,
      lastEventTimestamp: s.lastEventTimestamp,
      creationTime: s.creationTime,
    })),
  };
}

export async function getLogEvents(
  logGroupName: string,
  logStreamName: string,
  limit = 200,
): Promise<{ events: LogEvent[] }> {
  const data = await logsRequest<{ events?: RawLogEvent[] }>("GetLogEvents", {
    logGroupName,
    logStreamName,
    limit,
    startFromHead: false,
  });
  return {
    events: (data.events ?? []).map((e) => ({
      timestamp: e.timestamp ?? 0,
      message: e.message ?? "",
      ingestionTime: e.ingestionTime,
      eventId: e.eventId,
    })),
  };
}

export async function filterLogEvents(
  logGroupName: string,
  filterPattern: string,
  limit = 200,
): Promise<{ events: FilteredLogEvent[] }> {
  const body: Record<string, unknown> = { logGroupName, limit };
  if (filterPattern) body["filterPattern"] = filterPattern;
  const data = await logsRequest<{ events?: RawLogEvent[] }>(
    "FilterLogEvents",
    body,
  );
  return {
    events: (data.events ?? []).map((e) => ({
      timestamp: e.timestamp ?? 0,
      message: e.message ?? "",
      ingestionTime: e.ingestionTime,
      eventId: e.eventId,
      logStreamName: e.logStreamName ?? "",
    })),
  };
}

export async function startQuery(
  logGroupName: string,
  queryString: string,
  startTimeSecs: number,
  endTimeSecs: number,
  limit = 1000,
): Promise<{ queryId: string }> {
  const data = await logsRequest<{ queryId?: string }>("StartQuery", {
    logGroupName,
    startTime: startTimeSecs,
    endTime: endTimeSecs,
    queryString,
    limit,
  });
  return { queryId: data.queryId ?? "" };
}

export async function getQueryResults(
  queryId: string,
): Promise<InsightsQueryStatus> {
  const data = await logsRequest<{
    status?: string;
    results?: { field?: string; value?: string }[][];
    statistics?: {
      recordsMatched?: number;
      recordsScanned?: number;
      bytesScanned?: number;
    };
  }>("GetQueryResults", { queryId });
  return {
    queryId,
    status: data.status ?? "Unknown",
    results: (data.results ?? []).map((row) =>
      row.map((c) => ({ field: c.field ?? "", value: c.value ?? "" })),
    ),
    statistics: data.statistics,
  };
}
