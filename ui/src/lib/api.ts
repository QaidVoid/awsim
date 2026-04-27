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
