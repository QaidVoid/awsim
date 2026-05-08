/**
 * Client for the awsim built-in mock OIDC IdP admin endpoints under
 * `/_awsim/idp`. Lets users register, list, and remove mock providers
 * that real Cognito user-pool IdentityProviders of type OIDC can
 * point at for offline federation testing.
 */

const ENDPOINT = 'http://localhost:4566';

export interface MockIdpRegistration {
	provider_id: string;
	client_id: string;
	client_secret: string;
	discovery_url: string;
	authorize_url: string;
	token_url: string;
	userinfo_url: string;
	jwks_url: string;
	default_claims: Record<string, unknown>;
}

export interface MockIdpSummary {
	provider_id: string;
	client_id: string;
	default_claims: Record<string, unknown>;
}

/// Register a new mock provider. The body is optional: pass
/// `provider_id` to pin it to a stable string (default is a random
/// UUID), and `default_claims` to seed the claims-template the
/// authorize form pre-fills.
export async function registerMockIdp(input: {
	provider_id?: string;
	default_claims?: Record<string, unknown>;
}): Promise<MockIdpRegistration> {
	const res = await fetch(`${ENDPOINT}/_awsim/idp`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify(input)
	});
	const text = await res.text();
	if (!res.ok) throw new Error(`registerMockIdp failed: ${res.status} ${text}`);
	return JSON.parse(text);
}

export async function listMockIdps(): Promise<MockIdpSummary[]> {
	const res = await fetch(`${ENDPOINT}/_awsim/idp`);
	if (!res.ok) throw new Error(`listMockIdps failed: ${res.status}`);
	const data = (await res.json()) as { providers?: MockIdpSummary[] };
	return data.providers ?? [];
}

export async function deleteMockIdp(providerId: string): Promise<void> {
	const res = await fetch(`${ENDPOINT}/_awsim/idp/${encodeURIComponent(providerId)}`, {
		method: 'DELETE'
	});
	if (!res.ok && res.status !== 404)
		throw new Error(`deleteMockIdp failed: ${res.status}`);
}
