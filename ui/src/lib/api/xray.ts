/**
 * Typed AWS X-Ray API client. RestJson1 — paths like POST /TraceSummaries.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "xray";

export interface TraceSummary {
  id: string;
  duration: number;
  responseTime: number;
  hasError: boolean;
  hasFault: boolean;
  hasThrottle: boolean;
  isPartial: boolean;
  serviceNames: string[];
}

export interface TraceSegment {
  id: string;
  document: string;
}

export interface Trace {
  id: string;
  duration: number;
  segments: TraceSegment[];
}

export interface ServiceGraphNode {
  referenceId: number;
  name: string;
  state: string;
  okCount: number;
  errorTotalCount: number;
  faultTotalCount: number;
  totalCount: number;
}

function headers(): Record<string, string> {
  return {
    "Content-Type": "application/json",
    Authorization: authHeader(SERVICE),
    "X-Amz-Date": amzDate(),
  };
}

async function request<T>(
  action: string,
  path: string,
  body?: Record<string, unknown>,
): Promise<T> {
  const opts: RequestInit = { method: "POST", headers: headers() };
  if (body !== undefined) opts.body = JSON.stringify(body);
  const res = await loggedFetch(SERVICE, action, "POST", `${ENDPOINT}${path}`, opts);
  const text = await res.text();
  if (!res.ok) {
    let msg = text;
    try {
      const data = JSON.parse(text) as { message?: string; Message?: string };
      msg = data.message ?? data.Message ?? text;
    } catch {
      // not JSON
    }
    throw new Error(`X-Ray ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawSummary {
  Id: string;
  Duration: number;
  ResponseTime: number;
  HasError: boolean;
  HasFault: boolean;
  HasThrottle: boolean;
  IsPartial: boolean;
  ServiceIds?: Array<{ Name?: string }>;
}

interface RawTrace {
  Id: string;
  Duration: number;
  Segments?: Array<{ Id: string; Document: string }>;
}

interface RawServiceNode {
  ReferenceId: number;
  Name: string;
  State: string;
  SummaryStatistics?: {
    OkCount?: number;
    TotalCount?: number;
    ErrorStatistics?: { TotalCount?: number };
    FaultStatistics?: { TotalCount?: number };
  };
}

export async function getTraceSummaries(
  startTime: number,
  endTime: number,
): Promise<TraceSummary[]> {
  const data = await request<{ TraceSummaries?: RawSummary[] }>(
    "GetTraceSummaries",
    "/TraceSummaries",
    { StartTime: startTime, EndTime: endTime },
  );
  return (data.TraceSummaries ?? []).map((s) => ({
    id: s.Id,
    duration: s.Duration,
    responseTime: s.ResponseTime,
    hasError: s.HasError,
    hasFault: s.HasFault,
    hasThrottle: s.HasThrottle,
    isPartial: s.IsPartial,
    serviceNames: (s.ServiceIds ?? [])
      .map((sid) => sid.Name ?? "")
      .filter(Boolean),
  }));
}

export async function batchGetTraces(traceIds: string[]): Promise<Trace[]> {
  if (traceIds.length === 0) return [];
  const data = await request<{ Traces?: RawTrace[] }>(
    "BatchGetTraces",
    "/Traces",
    { TraceIds: traceIds },
  );
  return (data.Traces ?? []).map((t) => ({
    id: t.Id,
    duration: t.Duration,
    segments: (t.Segments ?? []).map((s) => ({
      id: s.Id,
      document: s.Document,
    })),
  }));
}

export async function getServiceGraph(
  startTime: number,
  endTime: number,
): Promise<ServiceGraphNode[]> {
  const data = await request<{ Services?: RawServiceNode[] }>(
    "GetServiceGraph",
    "/ServiceGraph",
    { StartTime: startTime, EndTime: endTime },
  );
  return (data.Services ?? []).map((n) => ({
    referenceId: n.ReferenceId,
    name: n.Name,
    state: n.State,
    okCount: n.SummaryStatistics?.OkCount ?? 0,
    errorTotalCount: n.SummaryStatistics?.ErrorStatistics?.TotalCount ?? 0,
    faultTotalCount: n.SummaryStatistics?.FaultStatistics?.TotalCount ?? 0,
    totalCount: n.SummaryStatistics?.TotalCount ?? 0,
  }));
}
