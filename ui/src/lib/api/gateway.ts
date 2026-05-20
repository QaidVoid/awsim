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
