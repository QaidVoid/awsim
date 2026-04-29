import type { StoragePayload } from "./events";

const BASE = "/_awsim";

export async function fetchHealth() {
  const res = await fetch(`${BASE}/health`);
  return res.json();
}

export async function fetchServices() {
  const res = await fetch(`${BASE}/services`);
  return res.json();
}

export async function fetchConfig() {
  const res = await fetch(`${BASE}/config`);
  return res.json();
}

export async function fetchStats() {
  const res = await fetch(`${BASE}/stats`);
  return res.json();
}

export async function fetchStorage(): Promise<StoragePayload> {
  const res = await fetch(`${BASE}/storage`);
  return res.json();
}

// ---------- SQLite-backed storage stats ----------

export interface SqliteStoreStats {
  service: string;
  /** `null` for services that don't expose a row count yet (DynamoDB). */
  rows: number | null;
  size_bytes: number;
}

export interface SqliteStatsPayload {
  stores: SqliteStoreStats[];
}

export async function fetchSqliteStats(): Promise<SqliteStatsPayload> {
  const res = await fetch(`${BASE}/storage/sqlite`);
  if (!res.ok) throw new Error(`SQLite stats fetch failed: ${res.status}`);
  return res.json();
}

// ---------- Billing ----------

export interface BillingDimension {
  description: string;
  price_per_request: number;
  request_count: number;
  cost_usd: number;
}

export interface BillingService {
  service: string;
  display_name: string;
  region: string;
  total_cost_usd: number;
  request_count: number;
  bytes_in: number;
  bytes_out: number;
  error_count: number;
  data_transfer_out_cost_usd: number;
  data_ingest_cost_usd: number;
  storage_cost_usd: number;
  storage_bytes: number;
  compute_cost_usd: number;
  compute_gb_seconds: number;
  resource_cost_usd: number;
  resource_count: number;
  dimensions: BillingDimension[];
}

export interface BillingReport {
  currency: string;
  elapsed_secs: number;
  running_cost_usd: number;
  projected_monthly_cost_usd: number;
  services: BillingService[];
}

export async function fetchBilling(): Promise<BillingReport> {
  const res = await fetch(`${BASE}/billing`);
  if (!res.ok) throw new Error(`Billing fetch failed: ${res.status}`);
  return res.json();
}

// ---------- Chaos ----------

export type ServiceMatch = { kind: "any" } | { kind: "exact"; value: string };
export type OperationMatch = { kind: "any" } | { kind: "exact"; value: string };

export interface ErrorEffect {
  status: number;
  code: string;
  message: string;
  retry_after_secs?: number;
}

export interface LatencyEffect {
  min_ms: number;
  max_ms: number;
}

export type ChaosEffect =
  | { kind: "error"; status: number; code: string; message: string; retry_after_secs?: number }
  | { kind: "latency"; min_ms: number; max_ms: number }
  | { kind: "both"; latency: LatencyEffect; error: ErrorEffect };

export interface TimeWindow {
  start_ts?: number;
  end_ts?: number;
}

export interface Flap {
  period_secs: number;
  active_secs: number;
  anchor_ts: number;
}

export interface ChaosSchedule {
  window?: TimeWindow;
  flap?: Flap;
}

export interface ChaosRule {
  id: string;
  service: ServiceMatch;
  operation: OperationMatch;
  probability: number;
  effect: ChaosEffect;
  enabled: boolean;
  label?: string | null;
  created_at: number;
  injection_count: number;
  schedule?: ChaosSchedule | null;
}

export interface ChaosRulesResponse {
  rules: ChaosRule[];
  total_injections: number;
}

export interface ChaosRecentInjection {
  ts: number;
  rule_id: string;
  service: string;
  operation?: string | null;
}

export interface ChaosStatsResponse {
  total_injections: number;
  recent: ChaosRecentInjection[];
}

export interface ChaosPresetInfo {
  name: string;
  description: string;
}

export async function fetchChaosRules(): Promise<ChaosRulesResponse> {
  const res = await fetch(`${BASE}/chaos/rules`);
  if (!res.ok) throw new Error(`Chaos rules fetch failed: ${res.status}`);
  return res.json();
}

export async function fetchChaosStats(): Promise<ChaosStatsResponse> {
  const res = await fetch(`${BASE}/chaos/stats`);
  if (!res.ok) throw new Error(`Chaos stats fetch failed: ${res.status}`);
  return res.json();
}

export async function fetchChaosPresets(): Promise<{ presets: ChaosPresetInfo[] }> {
  const res = await fetch(`${BASE}/chaos/presets`);
  if (!res.ok) throw new Error(`Chaos presets fetch failed: ${res.status}`);
  return res.json();
}

export async function applyChaosPreset(name: string): Promise<{ rule_ids: string[] }> {
  const res = await fetch(`${BASE}/chaos/presets/${encodeURIComponent(name)}`, {
    method: "POST",
  });
  if (!res.ok) throw new Error(`Apply preset failed: ${res.status}`);
  return res.json();
}

export async function addChaosRule(rule: Partial<ChaosRule>): Promise<{ id: string }> {
  const res = await fetch(`${BASE}/chaos/rules`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ id: "", enabled: true, ...rule }),
  });
  if (!res.ok) throw new Error(`Add rule failed: ${res.status} ${await res.text()}`);
  return res.json();
}

export async function setChaosRuleEnabled(id: string, enabled: boolean): Promise<void> {
  const res = await fetch(`${BASE}/chaos/rules/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ enabled }),
  });
  if (!res.ok) throw new Error(`Patch rule failed: ${res.status}`);
}

export async function removeChaosRule(id: string): Promise<void> {
  const res = await fetch(`${BASE}/chaos/rules/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
  if (!res.ok) throw new Error(`Delete rule failed: ${res.status}`);
}

export async function clearChaosRules(): Promise<void> {
  const res = await fetch(`${BASE}/chaos/clear`, { method: "POST" });
  if (!res.ok) throw new Error(`Clear failed: ${res.status}`);
}
