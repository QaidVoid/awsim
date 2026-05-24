/**
 * Operator auth API client.
 *
 * Mirrors the /_awsim/auth/* endpoints. The session is carried in
 * an HTTP-only awsim_session cookie set by the server, so the
 * browser sends it automatically and the JS layer never has to
 * handle the raw token. The whoami / login / logout helpers
 * return `null` when the operator is not signed in (or when
 * operator auth is off, in which case whoami 401s and the layout
 * stays loginless).
 */

const BASE = "/_awsim/auth";

export interface LoginRequest {
	username: string;
	password: string;
	mfa_code?: string;
}

export interface LoginResponse {
	session_token: string;
	expires_in: number;
	principal: string;
}

export interface WhoamiResponse {
	principal: string;
}

export interface SetupRequest {
	bootstrap_token: string;
	password: string;
}

export interface SetupResponse {
	principal: string;
	access_key_id: string;
	secret_access_key: string;
}

/** Status returned by login when something other than a 200 came back. */
export interface AuthError {
	code: string;
	message: string;
	status: number;
	retry_after?: number;
}

async function postJson<T>(path: string, body: unknown): Promise<T> {
	const res = await fetch(`${BASE}${path}`, {
		method: "POST",
		credentials: "same-origin",
		headers: { "content-type": "application/json" },
		body: JSON.stringify(body),
	});
	if (!res.ok) {
		const data = await safeJson(res);
		const retry = res.headers.get("retry-after");
		const err: AuthError = {
			code: typeof data?.code === "string" ? data.code : "Unknown",
			message:
				typeof data?.message === "string"
					? data.message
					: `Request failed: ${res.status}`,
			status: res.status,
			retry_after: retry ? Number(retry) : undefined,
		};
		throw err;
	}
	return res.json();
}

async function safeJson(res: Response): Promise<Record<string, unknown> | null> {
	try {
		return await res.json();
	} catch {
		return null;
	}
}

export async function login(req: LoginRequest): Promise<LoginResponse> {
	return postJson("/login", req);
}

export async function logout(): Promise<void> {
	await fetch(`${BASE}/logout`, {
		method: "POST",
		credentials: "same-origin",
	});
}

export interface WhoamiResult {
	authRequired: boolean;
	setupRequired: boolean;
	session: WhoamiResponse | null;
}

interface WhoamiBody {
	auth_required: boolean;
	setup_required: boolean;
	principal: string | null;
}

export async function whoami(): Promise<WhoamiResult> {
	const res = await fetch(`${BASE}/whoami`, { credentials: "same-origin" });
	if (!res.ok) {
		return { authRequired: false, setupRequired: false, session: null };
	}
	const body = (await res.json()) as WhoamiBody;
	return {
		authRequired: body.auth_required,
		setupRequired: body.setup_required,
		session: body.principal ? { principal: body.principal } : null,
	};
}

export async function setup(req: SetupRequest): Promise<SetupResponse> {
	return postJson("/setup", req);
}
