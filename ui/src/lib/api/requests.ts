/**
 * Request-inspector admin API.
 *
 * Wraps the `/_awsim/requests*` endpoints (recent-id list, single
 * captured request, replay) so components never call fetch directly.
 * The recent-ids lookup was previously duplicated raw across the
 * layout, command palette and playground.
 */

import type { RequestDetail } from "$lib/events";

/** Shape returned by the replay endpoint. */
export interface ReplayResult {
  new_id?: string;
  status_code?: number;
  error?: string;
  message?: string;
}

export type RequestDetailResult =
  | { ok: true; detail: RequestDetail }
  | { ok: false; status: number };

export interface ReplayResponse {
  ok: boolean;
  status: number;
  body: ReplayResult;
}

/**
 * Newest-first list of captured request ids from the in-memory ring
 * buffer. Throws `Error(String(status))` on a non-OK response so
 * callers can surface the status; best-effort callers may ignore it.
 */
export async function fetchRecentRequestIds(): Promise<string[]> {
  const res = await fetch("/_awsim/requests");
  if (!res.ok) throw new Error(String(res.status));
  const body = (await res.json()) as { ids?: string[] };
  return body.ids ?? [];
}

/**
 * Full detail for a captured request. Returns `{ ok: false, status }`
 * for HTTP errors (e.g. 404 once it rolls out of the buffer) so the
 * caller can craft the right message; network failures still throw.
 */
export async function fetchRequestDetail(
  id: string,
): Promise<RequestDetailResult> {
  const res = await fetch(`/_awsim/requests/${id}`);
  if (!res.ok) return { ok: false, status: res.status };
  return { ok: true, detail: (await res.json()) as RequestDetail };
}

/** Replay a captured request once. Never throws on HTTP errors -
 *  the status/body are returned for the caller to interpret. */
export async function replayRequest(id: string): Promise<ReplayResponse> {
  const res = await fetch(`/_awsim/requests/${id}/replay`, { method: "POST" });
  const body = (await res.json()) as ReplayResult;
  return { ok: res.ok, status: res.status, body };
}
