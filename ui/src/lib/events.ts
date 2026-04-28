/**
 * TypeScript shape for SSE events emitted from `/_awsim/events`.
 *
 * Mirrors `crates/awsim-core/src/request_event.rs::RequestEvent`. Keep
 * field names in sync with the Rust serde output (snake_case).
 */
export interface RequestEvent {
  id: string;
  /** Unix epoch seconds with fractional component. */
  ts: number;
  method: string;
  path: string;
  service: string;
  operation: string | null;
  account_id: string;
  region: string;
  principal_arn: string | null;
  status_code: number;
  duration_ms: number;
  request_size: number;
  response_size: number;
  error_code: string | null;
}

/**
 * Storage payload returned by `/_awsim/storage`. `data_dir` is null when
 * persistence is disabled, in which case `services` is also empty.
 */
export interface StoragePayload {
  data_dir: string | null;
  snapshots?: { path: string; size_bytes: number };
  services: StorageServiceEntry[];
  total_size_bytes?: number;
}

export interface StorageServiceEntry {
  name: string;
  groups: string[];
  size_bytes: number;
  blob_count: number;
}

/** Captured header (single name/value pair). Mirrors `CapturedHeader` in Rust. */
export interface CapturedHeader {
  name: string;
  value: string;
}

/**
 * Captured body slice. `data_b64` is null when the body was empty.
 * `truncated` is true when `data_b64` was capped below the original size.
 * Mirrors `CapturedBody` in Rust.
 */
export interface CapturedBody {
  data_b64: string | null;
  size: number;
  truncated: boolean;
}

/**
 * Full request detail returned by `/_awsim/requests/{id}`. Bodies are
 * size-capped on the server (default 64 KiB). Mirrors `RequestDetail`.
 */
export interface RequestDetail {
  id: string;
  method: string;
  path: string;
  query: string | null;
  status_code: number;
  request_headers: CapturedHeader[];
  response_headers: CapturedHeader[];
  request_body: CapturedBody;
  response_body: CapturedBody;
}
