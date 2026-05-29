/**
 * Model Gateway admin API. Phase 0 surfaces the bundled provider
 * catalog used to power the "Add backend" picker; further phases
 * extend this with credentials, aliases, health, and metrics.
 */

export type ProviderKind = "local" | "hosted" | "aws" | "custom";
export type AuthKind = "none" | "bearer";

export interface CatalogModel {
  id: string;
  context: number;
  modalities: string[];
  /** "chat" for completion / Converse targets, "embed" for embeddings. */
  kind: "chat" | "embed";
}

export interface CatalogProvider {
  key: string;
  name: string;
  /** Lucide icon name; see iconFor() in the gateway page for mapping. */
  icon: string;
  kind: ProviderKind;
  endpoint_template: string;
  auth: AuthKind;
  env_hint: string | null;
  docs_url: string | null;
  notes: string | null;
  models: CatalogModel[];
}

export interface ProviderCatalog {
  schema_version: number;
  providers: CatalogProvider[];
}

export async function getGatewayCatalog(): Promise<ProviderCatalog> {
  const res = await fetch("/_awsim/gateway/catalog");
  if (!res.ok) {
    throw new Error(`gateway/catalog failed (HTTP ${res.status})`);
  }
  return (await res.json()) as ProviderCatalog;
}

export type BackendStatus = "healthy" | "degraded" | "down" | "unknown";

export interface CheckRecord {
  at: string;
  latency_ms: number | null;
  error: string | null;
}

export interface BackendHealth {
  backend: string;
  status: BackendStatus;
  lastCheckedAt: string | null;
  lastLatencyMs: number | null;
  lastError: string | null;
  consecutiveFailures: number;
  consecutiveSuccesses: number;
  history: CheckRecord[];
}

export interface GatewayHealthResponse {
  backends: BackendHealth[];
}

export async function getGatewayHealth(): Promise<GatewayHealthResponse> {
  const res = await fetch("/_awsim/gateway/health");
  if (!res.ok) throw new Error(`gateway/health failed (HTTP ${res.status})`);
  return (await res.json()) as GatewayHealthResponse;
}

export interface RecheckResult {
  result: CheckRecord;
  backend: BackendHealth;
}

export type Outcome = "success" | "retriable" | "fatal";
export type OpKind = "chat" | "chat-stream" | "embed";

export interface MetricMappingRow {
  bedrockId: string;
  backend: string;
  success: number;
  retriable: number;
  fatal: number;
  total: number;
  p50Ms: number | null;
  p95Ms: number | null;
  lastError: string | null;
}

export interface MetricTotals {
  success: number;
  retriable: number;
  fatal: number;
  total: number;
}

export interface MetricsResponse {
  mappings: MetricMappingRow[];
  totals: MetricTotals;
}

export async function getGatewayMetrics(): Promise<MetricsResponse> {
  const res = await fetch("/_awsim/gateway/metrics");
  if (!res.ok) throw new Error(`gateway/metrics failed (HTTP ${res.status})`);
  return (await res.json()) as MetricsResponse;
}

export interface AttemptRecord {
  backend: string;
  tag: string;
  outcome: Outcome;
  latencyMs: number;
  error: string | null;
}

export interface InvocationRecord {
  at: string;
  bedrockId: string;
  op: OpKind;
  attempts: AttemptRecord[];
  outcome: Outcome;
  totalLatencyMs: number;
}

export interface RecentResponse {
  invocations: InvocationRecord[];
}

export async function getGatewayRecent(): Promise<RecentResponse> {
  const res = await fetch("/_awsim/gateway/recent");
  if (!res.ok) throw new Error(`gateway/recent failed (HTTP ${res.status})`);
  return (await res.json()) as RecentResponse;
}

/**
 * Token + cost summary lifted from a test-prompt's raw Converse
 * response when pricing is configured. Either side may be `null`
 * when the response carried tokens but no rate was set, which is
 * the typical Ollama-and-no-override case.
 */
export interface TestPromptUsage {
  inputTokens: number | null;
  outputTokens: number | null;
  totalTokens: number | null;
  inputCost: number | null;
  outputCost: number | null;
  totalCost: number | null;
}

export interface TestPromptResult {
  latencyMs: number;
  response: string | null;
  error: string | null;
  usage?: TestPromptUsage | null;
}

/**
 * Fires one Converse call through the live gateway. The call
 * goes through the same alias resolution / fallback / overrides
 * path real callers use, so the Activity tab will pick it up.
 */
export async function testGatewayPrompt(
  bedrockId: string,
  prompt: string,
): Promise<TestPromptResult> {
  const res = await fetch("/_awsim/gateway/test-prompt", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ bedrockId, prompt }),
  });
  if (!res.ok) {
    let msg = `gateway/test-prompt failed (HTTP ${res.status})`;
    try {
      const err = (await res.json()) as { message?: string };
      if (err.message) msg = err.message;
    } catch {
      /* fall through */
    }
    throw new Error(msg);
  }
  const parsed = (await res.json()) as TestPromptResult & {
    raw?: Record<string, unknown>;
  };
  const usage = parseUsageFromRaw(parsed.raw);
  return { ...parsed, usage };
}

function parseUsageFromRaw(
  raw: Record<string, unknown> | undefined,
): TestPromptUsage | null {
  const u = (raw?.["usage"] as Record<string, unknown> | undefined) ?? null;
  if (!u) return null;
  const num = (k: string): number | null => {
    const v = u[k];
    return typeof v === "number" && Number.isFinite(v) ? v : null;
  };
  return {
    inputTokens: num("inputTokens"),
    outputTokens: num("outputTokens"),
    totalTokens: num("totalTokens"),
    inputCost: num("input_cost"),
    outputCost: num("output_cost"),
    totalCost: num("total_cost"),
  };
}

export async function recheckGatewayBackend(name: string): Promise<RecheckResult> {
  const res = await fetch(
    `/_awsim/gateway/health/${encodeURIComponent(name)}/check`,
    { method: "POST" },
  );
  if (!res.ok) {
    let msg = `gateway/health/${name}/check failed (HTTP ${res.status})`;
    try {
      const err = (await res.json()) as { message?: string };
      if (err.message) msg = err.message;
    } catch {
      /* fall through */
    }
    throw new Error(msg);
  }
  return (await res.json()) as RecheckResult;
}
