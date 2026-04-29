/**
 * Typed EventBridge Pipes API client.
 *
 * AWSim exposes the same RestJson1 routes the real Pipes service uses:
 * /v1/pipes for CRUD, /v1/pipes/{Name}/start and /stop for lifecycle.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "pipes";

export type PipeState =
  | "CREATING"
  | "RUNNING"
  | "STOPPING"
  | "STOPPED"
  | "UPDATING"
  | "DELETING"
  | "CREATE_FAILED";

export interface PipeSummary {
  name: string;
  arn: string;
  source: string;
  target: string;
  currentState: PipeState;
  desiredState: PipeState;
  stateReason?: string;
  enrichment?: string;
  creationTime?: number;
  lastModifiedTime?: number;
}

export interface Pipe extends PipeSummary {
  roleArn: string;
  description?: string;
  sourceParameters?: Record<string, unknown>;
  targetParameters?: Record<string, unknown>;
  enrichmentParameters?: Record<string, unknown>;
  logConfiguration?: Record<string, unknown>;
  tags?: Record<string, string>;
}

export interface CreatePipeInput {
  name: string;
  source: string;
  target: string;
  roleArn: string;
  description?: string;
  desiredState?: "RUNNING" | "STOPPED";
  sourceParameters?: Record<string, unknown>;
  targetParameters?: Record<string, unknown>;
  enrichment?: string;
  enrichmentParameters?: Record<string, unknown>;
  tags?: Record<string, string>;
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
  method: "GET" | "POST" | "PUT" | "DELETE",
  path: string,
  body?: Record<string, unknown>,
): Promise<T> {
  const opts: RequestInit = { method, headers: headers() };
  if (body !== undefined) opts.body = JSON.stringify(body);
  const res = await loggedFetch(
    SERVICE,
    action,
    method,
    `${ENDPOINT}${path}`,
    opts,
  );
  const text = await res.text();
  if (!res.ok) {
    let msg = text;
    try {
      const data = JSON.parse(text) as { message?: string; Message?: string };
      msg = data.message ?? data.Message ?? text;
    } catch {
      // not JSON
    }
    throw new Error(`Pipes ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawPipeSummary {
  Name: string;
  Arn: string;
  Source: string;
  Target: string;
  CurrentState: PipeState;
  DesiredState: PipeState;
  StateReason?: string | null;
  Enrichment?: string | null;
  CreationTime?: number;
  LastModifiedTime?: number;
}

interface RawPipeDescribe extends RawPipeSummary {
  RoleArn: string;
  Description?: string | null;
  SourceParameters?: Record<string, unknown>;
  TargetParameters?: Record<string, unknown>;
  EnrichmentParameters?: Record<string, unknown>;
  LogConfiguration?: Record<string, unknown>;
  Tags?: Record<string, string>;
}

function fromRawSummary(r: RawPipeSummary): PipeSummary {
  return {
    name: r.Name,
    arn: r.Arn,
    source: r.Source,
    target: r.Target,
    currentState: r.CurrentState,
    desiredState: r.DesiredState,
    stateReason: r.StateReason ?? undefined,
    enrichment: r.Enrichment ?? undefined,
    creationTime: r.CreationTime,
    lastModifiedTime: r.LastModifiedTime,
  };
}

function fromRawDescribe(r: RawPipeDescribe): Pipe {
  return {
    ...fromRawSummary(r),
    roleArn: r.RoleArn,
    description: r.Description ?? undefined,
    sourceParameters: r.SourceParameters,
    targetParameters: r.TargetParameters,
    enrichmentParameters: r.EnrichmentParameters,
    logConfiguration: r.LogConfiguration,
    tags: r.Tags,
  };
}

export async function listPipes(filter?: {
  namePrefix?: string;
  sourcePrefix?: string;
  targetPrefix?: string;
  currentState?: PipeState;
  desiredState?: PipeState;
}): Promise<PipeSummary[]> {
  const params = new URLSearchParams();
  if (filter?.namePrefix) params.set("NamePrefix", filter.namePrefix);
  if (filter?.sourcePrefix) params.set("SourcePrefix", filter.sourcePrefix);
  if (filter?.targetPrefix) params.set("TargetPrefix", filter.targetPrefix);
  if (filter?.currentState) params.set("CurrentState", filter.currentState);
  if (filter?.desiredState) params.set("DesiredState", filter.desiredState);
  const qs = params.toString() ? `?${params.toString()}` : "";
  const data = await request<{ Pipes?: RawPipeSummary[] }>(
    "ListPipes",
    "GET",
    `/v1/pipes${qs}`,
  );
  return (data.Pipes ?? []).map(fromRawSummary);
}

export async function describePipe(name: string): Promise<Pipe> {
  const data = await request<RawPipeDescribe>(
    "DescribePipe",
    "GET",
    `/v1/pipes/${encodeURIComponent(name)}`,
  );
  return fromRawDescribe(data);
}

export async function createPipe(input: CreatePipeInput): Promise<PipeSummary> {
  const body: Record<string, unknown> = {
    Source: input.source,
    Target: input.target,
    RoleArn: input.roleArn,
    DesiredState: input.desiredState ?? "RUNNING",
  };
  if (input.description) body.Description = input.description;
  if (input.sourceParameters) body.SourceParameters = input.sourceParameters;
  if (input.targetParameters) body.TargetParameters = input.targetParameters;
  if (input.enrichment) body.Enrichment = input.enrichment;
  if (input.enrichmentParameters)
    body.EnrichmentParameters = input.enrichmentParameters;
  if (input.tags) body.Tags = input.tags;
  const data = await request<RawPipeSummary>(
    "CreatePipe",
    "POST",
    `/v1/pipes/${encodeURIComponent(input.name)}`,
    body,
  );
  return fromRawSummary(data);
}

export async function updatePipe(
  name: string,
  patch: Partial<CreatePipeInput>,
): Promise<PipeSummary> {
  const body: Record<string, unknown> = {};
  if (patch.target) body.Target = patch.target;
  if (patch.roleArn) body.RoleArn = patch.roleArn;
  if (patch.description !== undefined) body.Description = patch.description;
  if (patch.desiredState) body.DesiredState = patch.desiredState;
  if (patch.sourceParameters) body.SourceParameters = patch.sourceParameters;
  if (patch.targetParameters) body.TargetParameters = patch.targetParameters;
  if (patch.enrichment !== undefined) body.Enrichment = patch.enrichment;
  if (patch.enrichmentParameters)
    body.EnrichmentParameters = patch.enrichmentParameters;
  const data = await request<RawPipeSummary>(
    "UpdatePipe",
    "PUT",
    `/v1/pipes/${encodeURIComponent(name)}`,
    body,
  );
  return fromRawSummary(data);
}

export async function deletePipe(name: string): Promise<void> {
  await request<unknown>(
    "DeletePipe",
    "DELETE",
    `/v1/pipes/${encodeURIComponent(name)}`,
  );
}

export async function startPipe(name: string): Promise<PipeSummary> {
  const data = await request<RawPipeSummary>(
    "StartPipe",
    "POST",
    `/v1/pipes/${encodeURIComponent(name)}/start`,
    {},
  );
  return fromRawSummary(data);
}

export async function stopPipe(name: string): Promise<PipeSummary> {
  const data = await request<RawPipeSummary>(
    "StopPipe",
    "POST",
    `/v1/pipes/${encodeURIComponent(name)}/stop`,
    {},
  );
  return fromRawSummary(data);
}
