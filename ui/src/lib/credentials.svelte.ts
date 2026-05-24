/**
 * Singleton AWS credentials store for the admin UI.
 *
 * Holds the currently-active access key + secret used for signing
 * outbound AWS requests. Two modes:
 *
 *  - **Operator-authenticated**: after sign-in, the store fetches
 *    the operator's IAM credentials from `/_awsim/auth/credentials`
 *    and uses those. Refreshed automatically when the session
 *    changes via `auth.refresh()` (and lazily when within 5 minutes
 *    of expiry).
 *
 *  - **Loginless dev**: when operator auth is off or no session is
 *    present, falls back to the `awsim-admin` access key with the
 *    matching dev-mode secret. The server treats this access key as
 *    the admin short-circuit, so policy evaluation is skipped and
 *    the UI works without any auth setup.
 *
 * The fallback secret matches the literal the server reads from
 * `AWSIM_ADMIN_SECRET_ACCESS_KEY` (default in code:
 * "awsim-admin-secret"). The store never persists credentials
 * anywhere outside this module, and never exposes them to other
 * scripts: callers use `sign()` to get a SignedHeaders object back
 * without touching the raw secret.
 */

import { getCredentials, type OperatorCredentials } from "$lib/api/auth";
import { signRequest, type Credentials, type SignedHeaders } from "$lib/sigv4";

/**
 * Matches the default of the awsim binary's `--admin-access-key`
 * flag and the corresponding fixed admin secret. Real production
 * builds override both via env vars; the values here are the
 * loginless-dev fallback.
 */
const ADMIN_FALLBACK: Credentials = {
	accessKeyId: "awsim-admin",
	secretAccessKey: "awsim-admin-secret",
};

/** Refresh credentials when within this window of expiry. */
const REFRESH_WINDOW_MS = 5 * 60 * 1000;

class CredentialsStore {
	private operator = $state<OperatorCredentials | null>(null);
	private refreshing: Promise<void> | null = null;

	/** Refresh credentials from the server. Idempotent. */
	async refresh(): Promise<void> {
		if (this.refreshing) {
			return this.refreshing;
		}
		this.refreshing = (async () => {
			try {
				this.operator = await getCredentials();
			} catch {
				this.operator = null;
			} finally {
				this.refreshing = null;
			}
		})();
		return this.refreshing;
	}

	/** Drop cached operator credentials. Call on sign-out. */
	clear(): void {
		this.operator = null;
	}

	/**
	 * Pick the credentials to sign with right now. Falls back to the
	 * admin key when no operator session is established yet. Triggers
	 * a background refresh when within the refresh window of expiry,
	 * but never blocks the current request on it.
	 */
	current(): Credentials {
		if (this.operator) {
			const expiresAt = Date.parse(this.operator.expiresAt);
			if (Number.isFinite(expiresAt)) {
				const msToExpiry = expiresAt - Date.now();
				if (msToExpiry < REFRESH_WINDOW_MS) {
					// Fire-and-forget background refresh. Current
					// request still uses the cached credentials.
					void this.refresh();
				}
				if (msToExpiry <= 0) {
					this.operator = null;
				}
			}
		}
		if (this.operator) {
			return {
				accessKeyId: this.operator.accessKeyId,
				secretAccessKey: this.operator.secretAccessKey,
			};
		}
		return ADMIN_FALLBACK;
	}

	get principal(): string | null {
		return this.operator?.principal ?? null;
	}

	get usingAdminFallback(): boolean {
		return this.operator === null;
	}
}

export const credentials = new CredentialsStore();

/**
 * Sign an AWS request with the currently-active credentials.
 * Thin wrapper around [`signRequest`](./sigv4.ts) that pulls
 * credentials from the singleton store.
 */
export async function sign(
	method: string,
	url: string,
	region: string,
	service: string,
	body?: string | ArrayBuffer | Uint8Array,
	headers?: Record<string, string>,
): Promise<SignedHeaders> {
	return signRequest({
		method,
		url,
		region,
		service,
		body,
		headers,
		credentials: credentials.current(),
	});
}

/**
 * Install a global `fetch` wrapper that automatically re-signs any
 * AWS request leaving the page with real SigV4 using the
 * credentials store. Idempotent: calling twice is a no-op.
 *
 * Rather than touch every API client file to switch from the legacy
 * `authHeader()` placeholder to a real sign call, we intercept at
 * the network boundary. The wrapper:
 *
 *  - Detects AWS requests by host (matches the configured AWSim
 *    endpoint) and excludes the `/_awsim/*` admin paths, which use
 *    cookie-based session auth.
 *  - Extracts the AWS service name from the request's
 *    `Authorization` header (the legacy placeholder still includes
 *    it in the credential scope: `awsim-admin/.../<service>/...`)
 *    or from path heuristics for the handful of paths that omit it.
 *  - Re-signs with the operator's IAM credentials when available;
 *    falls back to the admin key otherwise.
 *
 * The trade-off vs touching every call site: one place to debug,
 * picks up future API additions for free, but the magic is
 * opaque. The on-the-wire result is identical to what the AWS SDKs
 * produce, so server-side SigV4 verification (Phase 3) sees
 * standard signatures regardless of which UI path emitted them.
 */
export function installFetchSigner(endpoint: string, defaultRegion = "us-east-1"): void {
	if (typeof window === "undefined") return;
	const w = window as typeof window & { __awsimFetchSignerInstalled?: boolean };
	if (w.__awsimFetchSignerInstalled) return;
	w.__awsimFetchSignerInstalled = true;

	const originalFetch = window.fetch.bind(window);
	const endpointHost = new URL(endpoint).host;

	window.fetch = async (
		input: RequestInfo | URL,
		init?: RequestInit,
	): Promise<Response> => {
		const { url, request } = normalizeFetchInput(input, init);
		// Pass through anything that isn't a request to the AWSim
		// AWS gateway. Admin endpoints under `/_awsim/*` are
		// session-cookie-authenticated and must not be signed.
		if (!isSignableAwsRequest(url, endpointHost)) {
			return originalFetch(input, init);
		}

		const method = request.method.toUpperCase();
		const incomingHeaders = headersToRecord(request.headers);
		const service =
			inferServiceFromHeaders(incomingHeaders) ?? inferServiceFromPath(url) ?? "s3";

		// Drop the placeholder auth header and the matching date.
		// The signer rebuilds both from scratch.
		delete incomingHeaders["authorization"];
		delete incomingHeaders["Authorization"];
		delete incomingHeaders["x-amz-date"];
		delete incomingHeaders["X-Amz-Date"];

		const body = await readBodyForSigning(init, request);
		const signed = await sign(method, url, defaultRegion, service, body, incomingHeaders);

		// The native fetch body type is narrowed by lib.dom to
		// disallow Uint8Array<SharedArrayBuffer>. Round-trip through
		// a fresh ArrayBuffer so the compiler accepts it; the wire
		// payload is identical.
		const outgoingBody: BodyInit | null | undefined =
			body.length === 0
				? (init?.body as BodyInit | null | undefined)
				: (body.buffer.slice(body.byteOffset, body.byteOffset + body.byteLength) as ArrayBuffer);
		return originalFetch(url, {
			...init,
			method,
			headers: signed,
			body: outgoingBody,
		});
	};
}

function normalizeFetchInput(
	input: RequestInfo | URL,
	init: RequestInit | undefined,
): { url: string; request: { method: string; headers: HeadersInit | undefined } } {
	if (typeof input === "string") {
		return { url: input, request: { method: init?.method ?? "GET", headers: init?.headers } };
	}
	if (input instanceof URL) {
		return {
			url: input.toString(),
			request: { method: init?.method ?? "GET", headers: init?.headers },
		};
	}
	return {
		url: input.url,
		request: { method: init?.method ?? input.method, headers: init?.headers ?? input.headers },
	};
}

function headersToRecord(headers: HeadersInit | undefined): Record<string, string> {
	const out: Record<string, string> = {};
	if (!headers) return out;
	if (headers instanceof Headers) {
		headers.forEach((v, k) => {
			out[k] = v;
		});
		return out;
	}
	if (Array.isArray(headers)) {
		for (const [k, v] of headers) out[k] = v;
		return out;
	}
	for (const [k, v] of Object.entries(headers)) {
		out[k] = v as string;
	}
	return out;
}

function isSignableAwsRequest(url: string, endpointHost: string): boolean {
	try {
		const u = new URL(url, window.location.origin);
		if (u.host !== endpointHost) return false;
		if (u.pathname.startsWith("/_awsim/")) return false;
		return true;
	} catch {
		return false;
	}
}

/**
 * Pull the service out of an `Authorization: AWS4-HMAC-SHA256
 * Credential=<key>/<date>/<region>/<service>/aws4_request, ...`
 * header. Returns null if the header isn't in the SigV4 shape.
 */
function inferServiceFromHeaders(headers: Record<string, string>): string | null {
	const auth = headers["authorization"] ?? headers["Authorization"];
	if (!auth) return null;
	const m = auth.match(/Credential=[^/]+\/[0-9]{8}\/[^/]+\/([^/]+)\/aws4_request/);
	return m ? m[1] : null;
}

/**
 * Fallback service detection for the few requests that go out
 * without an Authorization header. Path-prefix heuristics for the
 * REST-style services we know about; defaults handled by caller.
 */
function inferServiceFromPath(url: string): string | null {
	try {
		const u = new URL(url, window.location.origin);
		const p = u.pathname;
		if (p.startsWith("/2015-03-31/")) return "lambda";
		if (p.startsWith("/2014-03-28/")) return "logs";
		if (p.startsWith("/restapis")) return "apigateway";
		if (p.startsWith("/v2/email/")) return "ses";
		if (p.startsWith("/clusters")) return "eks";
		if (p.startsWith("/v1/")) return "iot";
	} catch {
		// fall through
	}
	return null;
}

async function readBodyForSigning(
	init: RequestInit | undefined,
	request: { method: string },
): Promise<Uint8Array> {
	if (request.method === "GET" || request.method === "HEAD") {
		return new Uint8Array();
	}
	const body = init?.body;
	if (body === undefined || body === null) return new Uint8Array();
	if (typeof body === "string") return new TextEncoder().encode(body);
	if (body instanceof Uint8Array) return body;
	if (body instanceof ArrayBuffer) return new Uint8Array(body);
	if (body instanceof Blob) {
		const ab = await body.arrayBuffer();
		return new Uint8Array(ab);
	}
	if (body instanceof URLSearchParams) {
		return new TextEncoder().encode(body.toString());
	}
	if (body instanceof FormData) {
		// SigV4 signs the encoded body; FormData's serialization
		// happens inside fetch and is multipart with a boundary the
		// caller doesn't see. For the operator UI this isn't a
		// common path (no service uses multipart from JS today), so
		// fall back to the unsigned-payload sentinel.
		return new TextEncoder().encode("UNSIGNED-PAYLOAD");
	}
	// ReadableStream and other exotic bodies: not used by the UI.
	return new Uint8Array();
}
