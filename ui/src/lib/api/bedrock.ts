/**
 * Typed Bedrock API client.
 *
 * Bedrock control-plane uses a REST-style API (`/foundation-models`,
 * `/guardrails`, `/custom-models`, ...). The runtime API
 * (`bedrock-runtime`) exposes `/model/{id}/invoke` for inference.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "bedrock";
const RUNTIME_SERVICE = "bedrock-runtime";

// ---------- Types ----------

export interface FoundationModel {
  modelId: string;
  modelArn: string;
  modelName: string;
  providerName: string;
  inputModalities: string[];
  outputModalities: string[];
  responseStreamingSupported?: boolean;
  customizationsSupported?: string[];
  inferenceTypesSupported?: string[];
}

export interface Guardrail {
  guardrailId: string;
  arn: string;
  name: string;
  description: string | null;
  status: string;
  createdAt: string | null;
  version: string;
}

export interface GuardrailDetail extends Guardrail {
  blockedInputMessaging: string;
  blockedOutputsMessaging: string;
  updatedAt: string | null;
}

export interface ProvisionedModelThroughput {
  provisionedModelArn: string;
  provisionedModelName: string;
  modelArn: string;
  status: string;
  desiredModelUnits: number;
  modelUnits: number;
  creationTime: string | null;
  commitmentDuration?: string | null;
}

export interface CustomModel {
  modelArn: string;
  modelName: string;
  baseModelArn: string;
  baseModelName: string;
  creationTime: string | null;
  customizationType?: string | null;
}

export interface CustomModelDetail extends CustomModel {
  jobArn?: string | null;
  jobName?: string | null;
  outputDataConfig?: { s3Uri: string } | null;
  trainingDataConfig?: { s3Uri: string } | null;
}

export interface KnowledgeBase {
  knowledgeBaseId: string;
  name: string;
  status: string;
  description: string | null;
  updatedAt: string | null;
}

export interface KnowledgeBaseDetail extends KnowledgeBase {
  roleArn: string | null;
  knowledgeBaseArn: string | null;
  createdAt: string | null;
}

export interface InvokeResult {
  body: string;
  contentType: string;
}

// ---------- Internal request helpers ----------

async function bedrockFetch<T>(
  method: string,
  path: string,
  service: string = SERVICE,
  body?: unknown,
): Promise<T> {
  const operation = `${method} ${path}`;
  const headers: Record<string, string> = {
    Authorization: authHeader(service),
    "X-Amz-Date": amzDate(),
  };
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const res = await loggedFetch(
    service,
    operation,
    method,
    `${ENDPOINT}${path}`,
    {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    },
  );
  if (!res.ok) {
    const text = await res.text();
    throw new Error(
      `Bedrock ${operation} failed (HTTP ${res.status}): ${text}`,
    );
  }
  const text = await res.text();
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawFoundationModel {
  modelId?: string;
  modelArn?: string;
  modelName?: string;
  providerName?: string;
  inputModalities?: string[];
  outputModalities?: string[];
  responseStreamingSupported?: boolean;
  customizationsSupported?: string[];
  inferenceTypesSupported?: string[];
}

interface RawGuardrail {
  id?: string;
  arn?: string;
  name?: string;
  description?: string | null;
  status?: string;
  createdAt?: string | null;
  version?: string;
}

interface RawGuardrailDetail extends RawGuardrail {
  guardrailId?: string;
  guardrailArn?: string;
  blockedInputMessaging?: string;
  blockedOutputsMessaging?: string;
  updatedAt?: string | null;
}

interface RawProvisionedModelThroughput {
  provisionedModelArn?: string;
  provisionedModelName?: string;
  modelArn?: string;
  status?: string;
  desiredModelUnits?: number;
  modelUnits?: number;
  creationTime?: string | null;
  commitmentDuration?: string | null;
}

interface RawCustomModel {
  modelArn?: string;
  modelName?: string;
  baseModelArn?: string;
  baseModelName?: string;
  creationTime?: string | null;
  customizationType?: string | null;
}

interface RawCustomModelDetail extends RawCustomModel {
  jobArn?: string | null;
  jobName?: string | null;
  outputDataConfig?: { s3Uri?: string } | null;
  trainingDataConfig?: { s3Uri?: string } | null;
}

interface RawKnowledgeBase {
  knowledgeBaseId?: string;
  name?: string;
  status?: string;
  description?: string | null;
  updatedAt?: string | null;
}

interface RawKnowledgeBaseDetail extends RawKnowledgeBase {
  roleArn?: string | null;
  knowledgeBaseArn?: string | null;
  createdAt?: string | null;
}

function mapFoundation(raw: RawFoundationModel): FoundationModel {
  return {
    modelId: raw.modelId ?? "",
    modelArn: raw.modelArn ?? "",
    modelName: raw.modelName ?? "",
    providerName: raw.providerName ?? "",
    inputModalities: raw.inputModalities ?? [],
    outputModalities: raw.outputModalities ?? [],
    responseStreamingSupported: raw.responseStreamingSupported ?? false,
    customizationsSupported: raw.customizationsSupported ?? [],
    inferenceTypesSupported: raw.inferenceTypesSupported ?? [],
  };
}

function mapGuardrail(raw: RawGuardrail): Guardrail {
  return {
    guardrailId: raw.id ?? "",
    arn: raw.arn ?? "",
    name: raw.name ?? "",
    description: raw.description ?? null,
    status: raw.status ?? "",
    createdAt: raw.createdAt ?? null,
    version: raw.version ?? "DRAFT",
  };
}

function mapGuardrailDetail(raw: RawGuardrailDetail): GuardrailDetail {
  return {
    guardrailId: raw.guardrailId ?? raw.id ?? "",
    arn: raw.guardrailArn ?? raw.arn ?? "",
    name: raw.name ?? "",
    description: raw.description ?? null,
    status: raw.status ?? "",
    createdAt: raw.createdAt ?? null,
    version: raw.version ?? "DRAFT",
    blockedInputMessaging: raw.blockedInputMessaging ?? "",
    blockedOutputsMessaging: raw.blockedOutputsMessaging ?? "",
    updatedAt: raw.updatedAt ?? null,
  };
}

function mapProvisioned(
  raw: RawProvisionedModelThroughput,
): ProvisionedModelThroughput {
  return {
    provisionedModelArn: raw.provisionedModelArn ?? "",
    provisionedModelName: raw.provisionedModelName ?? "",
    modelArn: raw.modelArn ?? "",
    status: raw.status ?? "",
    desiredModelUnits: raw.desiredModelUnits ?? 0,
    modelUnits: raw.modelUnits ?? 0,
    creationTime: raw.creationTime ?? null,
    commitmentDuration: raw.commitmentDuration ?? null,
  };
}

function mapCustomModel(raw: RawCustomModel): CustomModel {
  return {
    modelArn: raw.modelArn ?? "",
    modelName: raw.modelName ?? "",
    baseModelArn: raw.baseModelArn ?? "",
    baseModelName: raw.baseModelName ?? "",
    creationTime: raw.creationTime ?? null,
    customizationType: raw.customizationType ?? null,
  };
}

function mapCustomModelDetail(raw: RawCustomModelDetail): CustomModelDetail {
  return {
    ...mapCustomModel(raw),
    jobArn: raw.jobArn ?? null,
    jobName: raw.jobName ?? null,
    outputDataConfig: raw.outputDataConfig
      ? { s3Uri: raw.outputDataConfig.s3Uri ?? "" }
      : null,
    trainingDataConfig: raw.trainingDataConfig
      ? { s3Uri: raw.trainingDataConfig.s3Uri ?? "" }
      : null,
  };
}

function mapKnowledgeBase(raw: RawKnowledgeBase): KnowledgeBase {
  return {
    knowledgeBaseId: raw.knowledgeBaseId ?? "",
    name: raw.name ?? "",
    status: raw.status ?? "",
    description: raw.description ?? null,
    updatedAt: raw.updatedAt ?? null,
  };
}

function mapKnowledgeBaseDetail(
  raw: RawKnowledgeBaseDetail,
): KnowledgeBaseDetail {
  return {
    ...mapKnowledgeBase(raw),
    roleArn: raw.roleArn ?? null,
    knowledgeBaseArn: raw.knowledgeBaseArn ?? null,
    createdAt: raw.createdAt ?? null,
  };
}

// ---------- Operations: foundation models ----------

export async function listFoundationModels(): Promise<FoundationModel[]> {
  const res = await bedrockFetch<{ modelSummaries?: RawFoundationModel[] }>(
    "GET",
    "/foundation-models",
  );
  return (res.modelSummaries ?? []).map(mapFoundation);
}

// ---------- Operations: guardrails ----------

export async function listGuardrails(): Promise<Guardrail[]> {
  const res = await bedrockFetch<{ guardrails?: RawGuardrail[] }>(
    "GET",
    "/guardrails",
  );
  return (res.guardrails ?? []).map(mapGuardrail);
}

export async function getGuardrail(id: string): Promise<GuardrailDetail> {
  const res = await bedrockFetch<RawGuardrailDetail>(
    "GET",
    `/guardrails/${id}`,
  );
  return mapGuardrailDetail(res);
}

export async function createGuardrail(input: {
  name: string;
  blockedInputMessaging: string;
  blockedOutputsMessaging: string;
  description?: string;
}): Promise<{ guardrailId: string; arn: string }> {
  const res = await bedrockFetch<{
    guardrailId?: string;
    guardrailArn?: string;
  }>("POST", "/guardrails", SERVICE, input);
  return {
    guardrailId: res.guardrailId ?? "",
    arn: res.guardrailArn ?? "",
  };
}

export async function deleteGuardrail(id: string): Promise<void> {
  await bedrockFetch<unknown>("DELETE", `/guardrails/${id}`);
}

// ---------- Operations: provisioned throughput ----------

export async function listProvisionedModelThroughputs(): Promise<
  ProvisionedModelThroughput[]
> {
  const res = await bedrockFetch<{
    provisionedModelSummaries?: RawProvisionedModelThroughput[];
  }>("GET", "/provisioned-model-throughputs");
  return (res.provisionedModelSummaries ?? []).map(mapProvisioned);
}

// ---------- Operations: custom models ----------

export async function listCustomModels(): Promise<CustomModel[]> {
  const res = await bedrockFetch<{ modelSummaries?: RawCustomModel[] }>(
    "GET",
    "/custom-models",
  );
  return (res.modelSummaries ?? []).map(mapCustomModel);
}

export async function getCustomModel(id: string): Promise<CustomModelDetail> {
  const res = await bedrockFetch<RawCustomModelDetail>(
    "GET",
    `/custom-models/${id}`,
  );
  return mapCustomModelDetail(res);
}

// ---------- Operations: knowledge bases ----------

export async function listKnowledgeBases(): Promise<KnowledgeBase[]> {
  const res = await bedrockFetch<{
    knowledgeBaseSummaries?: RawKnowledgeBase[];
  }>("GET", "/knowledgebases");
  return (res.knowledgeBaseSummaries ?? []).map(mapKnowledgeBase);
}

export async function getKnowledgeBase(
  id: string,
): Promise<KnowledgeBaseDetail> {
  const res = await bedrockFetch<{ knowledgeBase?: RawKnowledgeBaseDetail }>(
    "GET",
    `/knowledgebases/${id}`,
  );
  return mapKnowledgeBaseDetail(res.knowledgeBase ?? {});
}

// ---------- Admin: bedrock proxy config ----------

export interface BedrockBackendInfo {
  name: string;
  endpoint: string;
  hasApiKey: boolean;
}

export interface BedrockModelMapEntry {
  id: string;
  tag: string;
  backend: string | null;
}

export interface BedrockProxyConfig {
  enabled: boolean;
  defaultBackend: string | null;
  backends: BedrockBackendInfo[];
  invoke: BedrockModelMapEntry[];
  embed: BedrockModelMapEntry[];
}

export interface BedrockDefaultsResponse {
  invoke: BedrockModelMapEntry[];
  embed: BedrockModelMapEntry[];
}

export async function getBedrockDefaults(): Promise<BedrockDefaultsResponse> {
  const res = await fetch("/_awsim/bedrock/defaults");
  if (!res.ok) throw new Error(`bedrock/defaults failed (HTTP ${res.status})`);
  return (await res.json()) as BedrockDefaultsResponse;
}

export interface BedrockBackendCheckResult {
  ok: boolean;
  latencyMs?: number;
  models?: string[];
  warning?: string;
  error?: string;
}

export async function checkBedrockBackend(
  name: string,
): Promise<BedrockBackendCheckResult> {
  const res = await fetch(
    `/_awsim/bedrock/backends/${encodeURIComponent(name)}/check`,
  );
  if (!res.ok) {
    return { ok: false, error: `HTTP ${res.status}` };
  }
  return (await res.json()) as BedrockBackendCheckResult;
}

export async function getBedrockProxyConfig(): Promise<BedrockProxyConfig> {
  const res = await fetch("/_awsim/bedrock/config");
  if (!res.ok) {
    throw new Error(`bedrock/config failed (HTTP ${res.status})`);
  }
  const raw = (await res.json()) as Partial<BedrockProxyConfig>;
  return {
    enabled: raw.enabled ?? false,
    defaultBackend: raw.defaultBackend ?? null,
    backends: raw.backends ?? [],
    invoke: raw.invoke ?? [],
    embed: raw.embed ?? [],
  };
}

// ---------- Runtime: invoke model ----------

export async function invokeModel(
  modelId: string,
  body: unknown,
  contentType = "application/json",
  accept = "application/json",
): Promise<InvokeResult> {
  const operation = `InvokeModel ${modelId}`;
  const path = `/model/${encodeURIComponent(modelId)}/invoke`;
  const res = await loggedFetch(
    RUNTIME_SERVICE,
    operation,
    "POST",
    `${ENDPOINT}${path}`,
    {
      method: "POST",
      headers: {
        "Content-Type": contentType,
        Accept: accept,
        Authorization: authHeader(RUNTIME_SERVICE),
        "X-Amz-Date": amzDate(),
      },
      body: typeof body === "string" ? body : JSON.stringify(body),
    },
  );
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`InvokeModel failed (HTTP ${res.status}): ${text}`);
  }
  const text = await res.text();
  return { body: text, contentType: res.headers.get("content-type") ?? accept };
}
