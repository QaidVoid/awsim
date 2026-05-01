/**
 * OpenSearch API client.
 *
 * AWSim implements the OpenSearch / Elasticsearch REST data plane
 * (no AWS control plane — no CreateDomain etc.) under the
 * `/opensearch/` URL prefix. Every call here is a plain HTTP request
 * — no SigV4 signing.
 */

const BASE = "/opensearch";

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
  // _cat returns plain text; everything else is JSON.
  if (path.startsWith("/_cat/")) return text as unknown as T;
  return JSON.parse(text) as T;
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
 * `_cat/indices` returns whitespace-aligned text rows. Parse into
 * structured rows so the UI can render a table.
 */
export async function listIndices(): Promise<IndexSummary[]> {
  // `?v` adds a header row; `?format=json` is more reliable.
  const data = await request<unknown>("GET", "/_cat/indices?format=json");
  const rows = (data as Array<Record<string, string>>) ?? [];
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
