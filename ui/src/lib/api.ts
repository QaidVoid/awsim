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
