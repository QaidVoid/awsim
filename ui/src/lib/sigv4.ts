/**
 * Browser-side AWS Signature Version 4 (SigV4) signer.
 *
 * Used by the admin UI to sign requests against the AWSim gateway
 * with the operator's IAM access key + secret. The output matches
 * what the AWS SDKs produce so the server can verify it via the
 * standard SigV4 algorithm (or in the loose mode, recognize the
 * Authorization header shape and trust the access key ID).
 *
 * Implements `AWS4-HMAC-SHA256` with the SubtleCrypto Web API so
 * there is no external dependency. The implementation follows the
 * canonical request -> string-to-sign -> derive signing key ->
 * signature pipeline documented at
 *
 *   https://docs.aws.amazon.com/general/latest/gr/signature-version-4.html
 *
 * What is NOT supported here:
 *  - Streaming payloads (`STREAMING-AWS4-HMAC-SHA256-PAYLOAD`).
 *    Browser bodies are small enough to hash fully.
 *  - Pre-signed URLs. We sign request headers only.
 *  - Session tokens beyond passing them as `X-Amz-Security-Token`.
 */

const ALGORITHM = "AWS4-HMAC-SHA256";

export interface Credentials {
	accessKeyId: string;
	secretAccessKey: string;
	/** Set only for STS-issued temporary credentials. */
	sessionToken?: string;
}

export interface SignedRequestInit {
	method: string;
	url: string;
	region: string;
	service: string;
	body?: string | ArrayBuffer | Uint8Array;
	headers?: Record<string, string>;
	credentials: Credentials;
}

export interface SignedHeaders {
	[name: string]: string;
}

const encoder = new TextEncoder();

/**
 * Sign a request and return the headers the caller should attach.
 * The original `headers` are merged into the result so the caller
 * can pass them verbatim to `fetch`.
 */
export async function signRequest(req: SignedRequestInit): Promise<SignedHeaders> {
	const url = new URL(req.url);
	const now = new Date();
	const amzDate = formatAmzDate(now);
	const dateStamp = amzDate.slice(0, 8);
	const method = req.method.toUpperCase();

	const bodyBytes = bodyToBytes(req.body);
	const payloadHash = await sha256Hex(bodyBytes);

	const baseHeaders: Record<string, string> = {
		...(req.headers ?? {}),
		host: url.host,
		"x-amz-date": amzDate,
		"x-amz-content-sha256": payloadHash,
	};
	if (req.credentials.sessionToken) {
		baseHeaders["x-amz-security-token"] = req.credentials.sessionToken;
	}

	const { canonicalHeaders, signedHeadersList } = canonicalize(baseHeaders);

	const canonicalRequest = [
		method,
		canonicalUri(url.pathname),
		canonicalQueryString(url.searchParams),
		canonicalHeaders,
		signedHeadersList,
		payloadHash,
	].join("\n");

	const credentialScope = `${dateStamp}/${req.region}/${req.service}/aws4_request`;
	const stringToSign = [
		ALGORITHM,
		amzDate,
		credentialScope,
		await sha256Hex(encoder.encode(canonicalRequest)),
	].join("\n");

	const signingKey = await deriveSigningKey(
		req.credentials.secretAccessKey,
		dateStamp,
		req.region,
		req.service,
	);
	const signature = await hmacHex(signingKey, encoder.encode(stringToSign));

	const authorization =
		`${ALGORITHM} ` +
		`Credential=${req.credentials.accessKeyId}/${credentialScope}, ` +
		`SignedHeaders=${signedHeadersList}, ` +
		`Signature=${signature}`;

	return {
		...baseHeaders,
		Authorization: authorization,
	};
}

function bodyToBytes(body: string | ArrayBuffer | Uint8Array | undefined): Uint8Array {
	if (body === undefined) return new Uint8Array();
	if (typeof body === "string") return encoder.encode(body);
	if (body instanceof Uint8Array) return body;
	return new Uint8Array(body);
}

/**
 * The canonical-URI rules: percent-encode each path segment per
 * RFC 3986 with the AWS-specific exception that `/` between
 * segments is preserved. S3 has an extra rule (no double-encoding)
 * we handle by encoding once.
 */
function canonicalUri(path: string): string {
	if (path === "" || path === "/") return "/";
	return path
		.split("/")
		.map((segment) => uriEncode(segment, false))
		.join("/");
}

function canonicalQueryString(params: URLSearchParams): string {
	const entries: [string, string][] = [];
	params.forEach((value, key) => {
		entries.push([key, value]);
	});
	entries.sort(([ak, av], [bk, bv]) => {
		if (ak < bk) return -1;
		if (ak > bk) return 1;
		if (av < bv) return -1;
		if (av > bv) return 1;
		return 0;
	});
	return entries
		.map(([k, v]) => `${uriEncode(k, true)}=${uriEncode(v, true)}`)
		.join("&");
}

/**
 * AWS uses RFC 3986 encoding with `=` and `&` always escaped in
 * query strings and `/` preserved in paths. encodeURIComponent gets
 * most of the way; we patch the differences.
 */
function uriEncode(value: string, encodeSlash: boolean): string {
	let out = encodeURIComponent(value).replace(
		/[!'()*]/g,
		(c) => "%" + c.charCodeAt(0).toString(16).toUpperCase(),
	);
	if (!encodeSlash) {
		out = out.replace(/%2F/g, "/");
	}
	return out;
}

function canonicalize(headers: Record<string, string>): {
	canonicalHeaders: string;
	signedHeadersList: string;
} {
	const entries = Object.entries(headers)
		.map(([name, value]) => [name.toLowerCase().trim(), value.trim().replace(/\s+/g, " ")] as const)
		.sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
	const canonicalHeaders = entries.map(([n, v]) => `${n}:${v}\n`).join("");
	const signedHeadersList = entries.map(([n]) => n).join(";");
	return { canonicalHeaders, signedHeadersList };
}

function formatAmzDate(d: Date): string {
	const iso = d.toISOString();
	return iso.replace(/[-:]/g, "").replace(/\.\d{3}Z$/, "Z");
}

async function sha256Hex(data: Uint8Array): Promise<string> {
	const digest = await crypto.subtle.digest("SHA-256", toArrayBuffer(data));
	return bufferToHex(digest);
}

async function hmac(key: ArrayBuffer | Uint8Array, data: Uint8Array): Promise<ArrayBuffer> {
	const keyBuf: BufferSource = key instanceof Uint8Array ? toArrayBuffer(key) : key;
	const cryptoKey = await crypto.subtle.importKey(
		"raw",
		keyBuf,
		{ name: "HMAC", hash: "SHA-256" },
		false,
		["sign"],
	);
	return crypto.subtle.sign("HMAC", cryptoKey, toArrayBuffer(data));
}

/**
 * Make a plain `ArrayBuffer` copy of a `Uint8Array`'s view. Needed
 * because TypeScript's lib.dom now narrows `BufferSource` to
 * `ArrayBuffer` (not `ArrayBufferLike`), so a `Uint8Array<SharedArrayBuffer>`
 * variant is rejected at type-check even though the runtime
 * accepts it.
 */
function toArrayBuffer(view: Uint8Array): ArrayBuffer {
	if (view.byteOffset === 0 && view.byteLength === view.buffer.byteLength && view.buffer instanceof ArrayBuffer) {
		return view.buffer;
	}
	const out = new ArrayBuffer(view.byteLength);
	new Uint8Array(out).set(view);
	return out;
}

async function hmacHex(key: ArrayBuffer | Uint8Array, data: Uint8Array): Promise<string> {
	return bufferToHex(await hmac(key, data));
}

async function deriveSigningKey(
	secret: string,
	dateStamp: string,
	region: string,
	service: string,
): Promise<ArrayBuffer> {
	const kSecret = encoder.encode(`AWS4${secret}`);
	const kDate = await hmac(kSecret, encoder.encode(dateStamp));
	const kRegion = await hmac(kDate, encoder.encode(region));
	const kService = await hmac(kRegion, encoder.encode(service));
	return hmac(kService, encoder.encode("aws4_request"));
}

function bufferToHex(buffer: ArrayBuffer): string {
	const bytes = new Uint8Array(buffer);
	let out = "";
	for (const b of bytes) {
		out += b.toString(16).padStart(2, "0");
	}
	return out;
}
