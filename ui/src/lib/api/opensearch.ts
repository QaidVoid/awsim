/**
 * OpenSearch API client.
 *
 * AWSim implements the OpenSearch / Elasticsearch REST data plane
 * (no AWS control plane — no CreateDomain etc.) under the
 * `/opensearch/` URL prefix on the awsim host. Every call here is a
 * plain HTTP request — no SigV4 signing.
 *
 * We use the absolute awsim endpoint (not a relative path) because
 * the UI is served at the same `/opensearch` route as a SvelteKit
 * page; a relative `fetch('/opensearch/...')` from the browser would
 * either hit the SPA HTML or 404 depending on the dev server, never
 * reaching the awsim API.
 */

import { ENDPOINT } from "$lib/aws";

const BASE = `${ENDPOINT}/opensearch`;

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
  contentType = "application/json",
): Promise<T> {
  const init: RequestInit = {
    method,
    headers: body !== undefined ? { "Content-Type": contentType } : undefined,
    body:
      body === undefined
        ? undefined
        : typeof body === "string"
          ? body
          : JSON.stringify(body),
  };
  const res = await fetch(`${BASE}${path}`, init);
  const text = await res.text();
  if (!res.ok) {
    throw new Error(`OpenSearch ${method} ${path} failed (${res.status}): ${text}`);
  }
  if (!text) return undefined as T;
  // Best-effort: try JSON, fall back to raw text. The _cat APIs
  // return plain text without `?format=json`; with the query param
  // (which we always send for cat) the response is JSON.
  try {
    return JSON.parse(text) as T;
  } catch {
    return text as unknown as T;
  }
}

export interface IndexSummary {
  health: string;
  status: string;
  index: string;
  uuid: string;
  pri: string;
  rep: string;
  docsCount: string;
  docsDeleted: string;
  storeSize: string;
  priStoreSize: string;
}

/**
 * `_cat/indices?format=json` returns an array of objects. Defensive
 * here because awsim's response shape may evolve and a non-array
 * would otherwise blow up the page with a `.map is not a function`
 * runtime error.
 */
export async function listIndices(): Promise<IndexSummary[]> {
  const data = await request<unknown>("GET", "/_cat/indices?format=json");
  const rows: Array<Record<string, string>> = Array.isArray(data)
    ? (data as Array<Record<string, string>>)
    : [];
  return rows.map((r) => ({
    health: r.health ?? "",
    status: r.status ?? "",
    index: r.index ?? "",
    uuid: r.uuid ?? "",
    pri: r.pri ?? "",
    rep: r.rep ?? "",
    docsCount: r["docs.count"] ?? "0",
    docsDeleted: r["docs.deleted"] ?? "0",
    storeSize: r["store.size"] ?? "0b",
    priStoreSize: r["pri.store.size"] ?? "0b",
  }));
}

export async function createIndex(name: string): Promise<void> {
  await request("PUT", `/${encodeURIComponent(name)}`, {});
}

export async function deleteIndex(name: string): Promise<void> {
  await request("DELETE", `/${encodeURIComponent(name)}`);
}

export async function getMapping(name: string): Promise<unknown> {
  return request("GET", `/${encodeURIComponent(name)}/_mapping`);
}

export async function clusterHealth(): Promise<{
  cluster_name: string;
  status: string;
  number_of_nodes: number;
  active_shards: number;
}> {
  return request("GET", "/_cluster/health");
}

// ---- Document ops ----

export interface SearchHit {
  _index: string;
  _id: string;
  _score: number | null;
  _source: Record<string, unknown>;
}

export interface SearchResult {
  hits: { total: { value: number }; hits: SearchHit[] };
  took?: number;
}

/**
 * Fire a search query against a single index. `query` is the raw
 * OpenSearch query DSL — caller supplies `{ "query": { "match_all": {} } }`
 * etc. Empty query string returns all docs.
 */
export async function search(
  index: string,
  body: unknown,
): Promise<SearchResult> {
  return request("POST", `/${encodeURIComponent(index)}/_search`, body);
}

export async function indexDocument(
  index: string,
  id: string,
  doc: Record<string, unknown>,
): Promise<void> {
  await request("PUT", `/${encodeURIComponent(index)}/_doc/${encodeURIComponent(id)}`, doc);
}

export async function deleteDocument(index: string, id: string): Promise<void> {
  await request(
    "DELETE",
    `/${encodeURIComponent(index)}/_doc/${encodeURIComponent(id)}`,
  );
}

export async function getDocument(
  index: string,
  id: string,
): Promise<{ _source: Record<string, unknown> } | null> {
  try {
    return await request("GET", `/${encodeURIComponent(index)}/_doc/${encodeURIComponent(id)}`);
  } catch (e) {
    // 404 surfaces as "failed (404)" — let the caller distinguish.
    if (e instanceof Error && /404/.test(e.message)) return null;
    throw e;
  }
}
