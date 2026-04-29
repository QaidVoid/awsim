/**
 * Lambda API client.
 *
 * Thin typed wrappers over the AWSim Lambda HTTP REST API
 * (`/2015-03-31/functions/...`) plus a CloudWatch Logs pull for the
 * `/aws/lambda/{name}` log group used by the logs tab.
 *
 * Every call is normalised to camel-cased, `undefined`-safe shapes so
 * the UI never has to think about the AWS wire format.
 */

const ENDPOINT = "http://localhost:4566";
const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, "");

function authHeader(service: string): string {
  return `AWS4-HMAC-SHA256 Credential=test/${FAKE_DATE}/us-east-1/${service}/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

function amzDate(): string {
  return new Date().toISOString().replace(/[:-]/g, "").slice(0, 15) + "Z";
}

function lambdaHeaders(): Record<string, string> {
  return {
    Authorization: authHeader("lambda"),
    "X-Amz-Date": amzDate(),
  };
}

function logsHeaders(): Record<string, string> {
  return {
    "Content-Type": "application/x-amz-json-1.1",
    "X-Amz-Target": "Logs_20140328",
    Authorization: authHeader("logs"),
    "X-Amz-Date": amzDate(),
  };
}

async function lambdaFetch(
  method: "GET" | "POST" | "PUT" | "DELETE",
  path: string,
  body?: unknown,
): Promise<Response> {
  const init: RequestInit = {
    method,
    headers: {
      ...lambdaHeaders(),
      ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
    },
  };
  if (body !== undefined) {
    init.body = typeof body === "string" ? body : JSON.stringify(body);
  }
  const res = await fetch(`${ENDPOINT}${path}`, init);
  return res;
}

async function ok<T>(res: Response): Promise<T> {
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`HTTP ${res.status}: ${text || res.statusText}`);
  }
  // Some operations (DELETE) return empty bodies.
  const text = await res.text();
  if (!text) return {} as T;
  return JSON.parse(text) as T;
}

// -- Types --

export interface LambdaFunction {
  name: string;
  arn: string;
  runtime: string;
  handler: string;
  role: string;
  memorySize: number;
  timeout: number;
  codeSize: number;
  description: string;
  lastModified: string;
  version: string;
}

export interface LambdaConfiguration extends LambdaFunction {
  envVars: Record<string, string>;
  state: string;
}

export interface LambdaCodeLocation {
  repositoryType: string;
  location: string;
}

export interface LambdaFunctionDetail {
  configuration: LambdaConfiguration;
  code?: LambdaCodeLocation;
}

export interface LambdaVersion {
  version: string;
  description: string;
  lastModified: string;
  codeSize: number;
}

export interface InvokeResult {
  statusCode: number;
  payload: string;
  logTail: string | null;
  durationMs: number;
  functionError: string | null;
  executedVersion: string | null;
}

export interface LogEvent {
  timestamp: number;
  message: string;
  ingestionTime?: number;
}

interface RawFunctionConfig {
  FunctionName?: string;
  FunctionArn?: string;
  Runtime?: string;
  Handler?: string;
  Role?: string;
  MemorySize?: number;
  Timeout?: number;
  CodeSize?: number;
  Description?: string;
  LastModified?: string;
  Version?: string;
  State?: string;
  Environment?: { Variables?: Record<string, string> };
}

interface RawListFunctionsResponse {
  Functions?: RawFunctionConfig[];
}

interface RawGetFunctionResponse {
  Configuration?: RawFunctionConfig;
  Code?: { RepositoryType?: string; Location?: string };
}

interface RawListVersionsResponse {
  Versions?: RawFunctionConfig[];
}

function mapConfig(raw: RawFunctionConfig | undefined): LambdaConfiguration {
  return {
    name: raw?.FunctionName ?? "",
    arn: raw?.FunctionArn ?? "",
    runtime: raw?.Runtime ?? "",
    handler: raw?.Handler ?? "",
    role: raw?.Role ?? "",
    memorySize: raw?.MemorySize ?? 0,
    timeout: raw?.Timeout ?? 0,
    codeSize: raw?.CodeSize ?? 0,
    description: raw?.Description ?? "",
    lastModified: raw?.LastModified ?? "",
    version: raw?.Version ?? "$LATEST",
    state: raw?.State ?? "",
    envVars: raw?.Environment?.Variables ?? {},
  };
}

// -- Operations --

export async function listFunctions(): Promise<{
  functions: LambdaFunction[];
}> {
  const res = await lambdaFetch("GET", `/2015-03-31/functions`);
  const data = await ok<RawListFunctionsResponse>(res);
  return {
    functions: (data.Functions ?? []).map(mapConfig),
  };
}

export async function getFunction(name: string): Promise<LambdaFunctionDetail> {
  const res = await lambdaFetch(
    "GET",
    `/2015-03-31/functions/${encodeURIComponent(name)}`,
  );
  const data = await ok<RawGetFunctionResponse>(res);
  return {
    configuration: mapConfig(data.Configuration),
    code: data.Code
      ? {
          repositoryType: data.Code.RepositoryType ?? "",
          location: data.Code.Location ?? "",
        }
      : undefined,
  };
}

export async function getFunctionConfiguration(
  name: string,
): Promise<LambdaConfiguration> {
  const res = await lambdaFetch(
    "GET",
    `/2015-03-31/functions/${encodeURIComponent(name)}/configuration`,
  );
  const data = await ok<RawFunctionConfig>(res);
  return mapConfig(data);
}

export interface UpdateConfigurationInput {
  runtime?: string;
  handler?: string;
  memorySize?: number;
  timeout?: number;
  description?: string;
  envVars?: Record<string, string>;
}

export async function updateFunctionConfiguration(
  name: string,
  input: UpdateConfigurationInput,
): Promise<LambdaConfiguration> {
  const body: Record<string, unknown> = {};
  if (input.runtime) body["Runtime"] = input.runtime;
  if (input.handler) body["Handler"] = input.handler;
  if (input.memorySize !== undefined) body["MemorySize"] = input.memorySize;
  if (input.timeout !== undefined) body["Timeout"] = input.timeout;
  if (input.description !== undefined) body["Description"] = input.description;
  if (input.envVars !== undefined) {
    body["Environment"] = { Variables: input.envVars };
  }
  const res = await lambdaFetch(
    "PUT",
    `/2015-03-31/functions/${encodeURIComponent(name)}/configuration`,
    body,
  );
  const data = await ok<RawFunctionConfig>(res);
  return mapConfig(data);
}

export interface CreateFunctionInput {
  name: string;
  runtime: string;
  handler: string;
  role: string;
  zipFileBase64?: string;
  description?: string;
  memorySize?: number;
  timeout?: number;
  envVars?: Record<string, string>;
}

export async function createFunction(
  input: CreateFunctionInput,
): Promise<void> {
  const body: Record<string, unknown> = {
    FunctionName: input.name,
    Runtime: input.runtime,
    Handler: input.handler,
    Role: input.role,
    Code: { ZipFile: input.zipFileBase64 ?? "" },
  };
  if (input.description) body["Description"] = input.description;
  if (input.memorySize !== undefined) body["MemorySize"] = input.memorySize;
  if (input.timeout !== undefined) body["Timeout"] = input.timeout;
  if (input.envVars && Object.keys(input.envVars).length > 0) {
    body["Environment"] = { Variables: input.envVars };
  }
  const res = await lambdaFetch("POST", `/2015-03-31/functions`, body);
  await ok(res);
}

export async function deleteFunction(name: string): Promise<void> {
  const res = await lambdaFetch(
    "DELETE",
    `/2015-03-31/functions/${encodeURIComponent(name)}`,
  );
  await ok(res);
}

export async function invokeFunction(
  name: string,
  payload: string,
): Promise<InvokeResult> {
  const start = Date.now();
  const res = await fetch(
    `${ENDPOINT}/2015-03-31/functions/${encodeURIComponent(name)}/invocations`,
    {
      method: "POST",
      headers: {
        ...lambdaHeaders(),
        "Content-Type": "application/json",
        "X-Amz-Log-Type": "Tail",
      },
      body: payload || "{}",
    },
  );
  const durationMs = Date.now() - start;
  const text = await res.text();
  const logHeader =
    res.headers.get("X-Amz-Log-Result") ?? res.headers.get("x-amz-log-result");
  let logTail: string | null = null;
  if (logHeader) {
    try {
      logTail = atob(logHeader);
    } catch {
      logTail = logHeader;
    }
  }
  return {
    statusCode: res.status,
    payload: text,
    logTail,
    durationMs,
    functionError:
      res.headers.get("X-Amz-Function-Error") ??
      res.headers.get("x-amz-function-error"),
    executedVersion:
      res.headers.get("X-Amz-Executed-Version") ??
      res.headers.get("x-amz-executed-version"),
  };
}

export async function listVersionsByFunction(
  name: string,
): Promise<{ versions: LambdaVersion[] }> {
  const res = await lambdaFetch(
    "GET",
    `/2015-03-31/functions/${encodeURIComponent(name)}/versions`,
  );
  const data = await ok<RawListVersionsResponse>(res);
  return {
    versions: (data.Versions ?? []).map((v) => ({
      version: v.Version ?? "$LATEST",
      description: v.Description ?? "",
      lastModified: v.LastModified ?? "",
      codeSize: v.CodeSize ?? 0,
    })),
  };
}

export async function publishVersion(
  name: string,
  description?: string,
): Promise<LambdaVersion> {
  const body: Record<string, unknown> = {};
  if (description) body["Description"] = description;
  const res = await lambdaFetch(
    "POST",
    `/2015-03-31/functions/${encodeURIComponent(name)}/versions`,
    body,
  );
  const data = await ok<RawFunctionConfig>(res);
  return {
    version: data.Version ?? "",
    description: data.Description ?? "",
    lastModified: data.LastModified ?? "",
    codeSize: data.CodeSize ?? 0,
  };
}

// ---- CloudWatch Logs (tail of /aws/lambda/{name}) ----

interface RawLogStream {
  logStreamName?: string;
  lastEventTimestamp?: number;
}

interface RawLogStreamsResponse {
  logStreams?: RawLogStream[];
}

interface RawLogEvent {
  timestamp?: number;
  message?: string;
  ingestionTime?: number;
}

interface RawLogEventsResponse {
  events?: RawLogEvent[];
}

async function logsRequest<T>(
  action: string,
  body: Record<string, unknown>,
): Promise<T> {
  const res = await fetch(`${ENDPOINT}/`, {
    method: "POST",
    headers: {
      ...logsHeaders(),
      "X-Amz-Target": `Logs_20140328.${action}`,
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

export async function tailLogs(
  functionName: string,
  limit = 100,
): Promise<{ events: LogEvent[] }> {
  const logGroupName = `/aws/lambda/${functionName}`;
  let streams: RawLogStream[] = [];
  try {
    const data = await logsRequest<RawLogStreamsResponse>(
      "DescribeLogStreams",
      {
        logGroupName,
        orderBy: "LastEventTime",
        descending: true,
        limit: 5,
      },
    );
    streams = data.logStreams ?? [];
  } catch {
    return { events: [] };
  }
  if (streams.length === 0) return { events: [] };

  const events: LogEvent[] = [];
  for (const s of streams) {
    if (!s.logStreamName) continue;
    try {
      const data = await logsRequest<RawLogEventsResponse>("GetLogEvents", {
        logGroupName,
        logStreamName: s.logStreamName,
        limit,
        startFromHead: false,
      });
      for (const e of data.events ?? []) {
        events.push({
          timestamp: e.timestamp ?? 0,
          message: e.message ?? "",
          ingestionTime: e.ingestionTime,
        });
      }
    } catch {
      // skip stream on error
    }
  }
  events.sort((a, b) => b.timestamp - a.timestamp);
  return { events: events.slice(0, limit) };
}

// -- Concurrency --

export interface ProvisionedConcurrencyConfig {
  qualifier: string;
  requestedProvisionedConcurrentExecutions: number;
  allocatedProvisionedConcurrentExecutions: number;
  availableProvisionedConcurrentExecutions: number;
  status: string;
  statusReason?: string;
  lastModified: string;
}

export async function getFunctionConcurrency(
  name: string,
): Promise<{ reservedConcurrentExecutions?: number }> {
  const res = await lambdaFetch(
    "GET",
    `/2019-09-30/functions/${encodeURIComponent(name)}/concurrency`,
  );
  const body = await ok<{ ReservedConcurrentExecutions?: number }>(res);
  return { reservedConcurrentExecutions: body.ReservedConcurrentExecutions };
}

export async function putFunctionConcurrency(
  name: string,
  reserved: number,
): Promise<void> {
  const res = await lambdaFetch(
    "PUT",
    `/2017-10-31/functions/${encodeURIComponent(name)}/concurrency`,
    { ReservedConcurrentExecutions: reserved },
  );
  await ok(res);
}

export async function deleteFunctionConcurrency(name: string): Promise<void> {
  const res = await lambdaFetch(
    "DELETE",
    `/2017-10-31/functions/${encodeURIComponent(name)}/concurrency`,
  );
  await ok(res);
}

export async function listProvisionedConcurrencyConfigs(
  name: string,
): Promise<ProvisionedConcurrencyConfig[]> {
  const res = await lambdaFetch(
    "GET",
    `/2019-09-30/functions/${encodeURIComponent(name)}/provisioned-concurrency`,
  );
  const body = await ok<{
    ProvisionedConcurrencyConfigs?: Array<{
      FunctionArn?: string;
      RequestedProvisionedConcurrentExecutions: number;
      AllocatedProvisionedConcurrentExecutions: number;
      AvailableProvisionedConcurrentExecutions: number;
      Status: string;
      StatusReason?: string;
      LastModified: string;
    }>;
  }>(res);
  return (body.ProvisionedConcurrencyConfigs ?? []).map((c) => ({
    qualifier: extractQualifier(c.FunctionArn ?? ""),
    requestedProvisionedConcurrentExecutions:
      c.RequestedProvisionedConcurrentExecutions,
    allocatedProvisionedConcurrentExecutions:
      c.AllocatedProvisionedConcurrentExecutions,
    availableProvisionedConcurrentExecutions:
      c.AvailableProvisionedConcurrentExecutions,
    status: c.Status,
    statusReason: c.StatusReason,
    lastModified: c.LastModified,
  }));
}

function extractQualifier(arn: string): string {
  // arn:aws:lambda:region:account:function:name:qualifier — the last segment
  // is the qualifier when present.
  const parts = arn.split(":");
  return parts[parts.length - 1] ?? "";
}

export async function putProvisionedConcurrencyConfig(
  name: string,
  qualifier: string,
  count: number,
): Promise<void> {
  const path =
    `/2019-09-30/functions/${encodeURIComponent(name)}/provisioned-concurrency` +
    `?Qualifier=${encodeURIComponent(qualifier)}`;
  const res = await lambdaFetch("PUT", path, {
    ProvisionedConcurrentExecutions: count,
  });
  await ok(res);
}

export async function deleteProvisionedConcurrencyConfig(
  name: string,
  qualifier: string,
): Promise<void> {
  const path =
    `/2019-09-30/functions/${encodeURIComponent(name)}/provisioned-concurrency` +
    `?Qualifier=${encodeURIComponent(qualifier)}`;
  const res = await lambdaFetch("DELETE", path);
  await ok(res);
}

// ----- Event source mappings -----

export interface EventSourceMappingSummary {
  uuid: string;
  eventSourceArn: string;
  functionArn: string;
  state: string;
  batchSize: number;
  maximumBatchingWindowInSeconds: number;
  startingPosition?: string;
  bisectBatchOnFunctionError: boolean;
  maximumRetryAttempts?: number;
  parallelizationFactor?: number;
  filterCriteria?: { Filters?: { Pattern: string }[] };
  destinationOnFailure?: string;
  lastProcessingResult: string;
  lastModified: string;
}

export interface CreateEventSourceMappingInput {
  functionName: string;
  eventSourceArn: string;
  batchSize?: number;
  enabled?: boolean;
  startingPosition?: string;
  maximumBatchingWindowInSeconds?: number;
  filterPatternJson?: string;
  destinationOnFailureArn?: string;
}

interface RawEsm {
  UUID: string;
  EventSourceArn: string;
  FunctionArn: string;
  State: string;
  BatchSize: number;
  MaximumBatchingWindowInSeconds?: number;
  StartingPosition?: string;
  BisectBatchOnFunctionError?: boolean;
  MaximumRetryAttempts?: number;
  ParallelizationFactor?: number;
  FilterCriteria?: { Filters?: { Pattern: string }[] };
  DestinationConfig?: { OnFailure?: { Destination?: string } };
  LastProcessingResult?: string;
  LastModified?: string;
}

function fromRawEsm(r: RawEsm): EventSourceMappingSummary {
  return {
    uuid: r.UUID,
    eventSourceArn: r.EventSourceArn,
    functionArn: r.FunctionArn,
    state: r.State,
    batchSize: r.BatchSize,
    maximumBatchingWindowInSeconds: r.MaximumBatchingWindowInSeconds ?? 0,
    startingPosition: r.StartingPosition,
    bisectBatchOnFunctionError: r.BisectBatchOnFunctionError ?? false,
    maximumRetryAttempts: r.MaximumRetryAttempts,
    parallelizationFactor: r.ParallelizationFactor,
    filterCriteria: r.FilterCriteria,
    destinationOnFailure: r.DestinationConfig?.OnFailure?.Destination,
    lastProcessingResult: r.LastProcessingResult ?? "",
    lastModified: r.LastModified ?? "",
  };
}

export async function listEventSourceMappings(
  functionName?: string,
): Promise<EventSourceMappingSummary[]> {
  const qs = functionName
    ? `?FunctionName=${encodeURIComponent(functionName)}`
    : "";
  const res = await lambdaFetch("GET", `/2015-03-31/event-source-mappings${qs}`);
  await ok(res);
  const data = (await res.json()) as { EventSourceMappings?: RawEsm[] };
  return (data.EventSourceMappings ?? []).map(fromRawEsm);
}

export async function createEventSourceMapping(
  input: CreateEventSourceMappingInput,
): Promise<EventSourceMappingSummary> {
  const body: Record<string, unknown> = {
    FunctionName: input.functionName,
    EventSourceArn: input.eventSourceArn,
    Enabled: input.enabled ?? true,
  };
  if (input.batchSize !== undefined) body.BatchSize = input.batchSize;
  if (input.maximumBatchingWindowInSeconds !== undefined)
    body.MaximumBatchingWindowInSeconds = input.maximumBatchingWindowInSeconds;
  if (input.startingPosition) body.StartingPosition = input.startingPosition;
  if (input.filterPatternJson?.trim()) {
    body.FilterCriteria = {
      Filters: [{ Pattern: input.filterPatternJson.trim() }],
    };
  }
  if (input.destinationOnFailureArn?.trim()) {
    body.DestinationConfig = {
      OnFailure: { Destination: input.destinationOnFailureArn.trim() },
    };
  }
  const res = await lambdaFetch("POST", `/2015-03-31/event-source-mappings`, body);
  await ok(res);
  return fromRawEsm((await res.json()) as RawEsm);
}

export async function updateEventSourceMapping(
  uuid: string,
  patch: {
    enabled?: boolean;
    batchSize?: number;
    maximumBatchingWindowInSeconds?: number;
    filterPatternJson?: string | null;
    destinationOnFailureArn?: string | null;
  },
): Promise<EventSourceMappingSummary> {
  const body: Record<string, unknown> = {};
  if (patch.enabled !== undefined) body.Enabled = patch.enabled;
  if (patch.batchSize !== undefined) body.BatchSize = patch.batchSize;
  if (patch.maximumBatchingWindowInSeconds !== undefined)
    body.MaximumBatchingWindowInSeconds = patch.maximumBatchingWindowInSeconds;
  if (patch.filterPatternJson !== undefined) {
    body.FilterCriteria = patch.filterPatternJson
      ? { Filters: [{ Pattern: patch.filterPatternJson }] }
      : { Filters: [] };
  }
  if (patch.destinationOnFailureArn !== undefined) {
    body.DestinationConfig = patch.destinationOnFailureArn
      ? { OnFailure: { Destination: patch.destinationOnFailureArn } }
      : { OnFailure: {} };
  }
  const res = await lambdaFetch(
    "PUT",
    `/2015-03-31/event-source-mappings/${encodeURIComponent(uuid)}`,
    body,
  );
  await ok(res);
  return fromRawEsm((await res.json()) as RawEsm);
}

export async function deleteEventSourceMapping(uuid: string): Promise<void> {
  const res = await lambdaFetch(
    "DELETE",
    `/2015-03-31/event-source-mappings/${encodeURIComponent(uuid)}`,
  );
  await ok(res);
}
