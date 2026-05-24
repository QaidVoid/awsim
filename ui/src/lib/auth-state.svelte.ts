/**
 * Reactive session state for the admin UI.
 *
 * Holds the currently-signed-in operator principal, populated by
 * a `whoami` probe in the root layout. `null` means no session;
 * `undefined` means not yet probed. The layout watches this and
 * redirects to /login when it flips to null while operator auth
 * is on.
 */

import { whoami, logout as apiLogout } from "$lib/api/auth";

export interface AuthSession {
	principal: string;
}

class AuthStore {
	session = $state<AuthSession | null | undefined>(undefined);
	setupRequired = $state(false);

	async refresh(): Promise<void> {
		const result = await whoami();
		this.session = result.session;
		this.setupRequired = result.setupRequired;
	}

	async signOut(): Promise<void> {
		await apiLogout();
		this.session = null;
	}

	get loaded(): boolean {
		return this.session !== undefined;
	}

	get signedIn(): boolean {
		return !!this.session;
	}

	get displayName(): string {
		if (!this.session) return "";
		const principal = this.session.principal;
		const slash = principal.lastIndexOf("/");
		return slash >= 0 ? principal.slice(slash + 1) : principal;
	}
}

export const auth = new AuthStore();
