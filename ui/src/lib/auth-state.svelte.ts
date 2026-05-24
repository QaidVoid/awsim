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
import { credentials } from "$lib/credentials.svelte";

export interface AuthSession {
	principal: string;
}

class AuthStore {
	session = $state<AuthSession | null | undefined>(undefined);
	authRequired = $state(false);
	setupRequired = $state(false);

	async refresh(): Promise<void> {
		const result = await whoami();
		this.authRequired = result.authRequired;
		this.setupRequired = result.setupRequired;
		this.session = result.session;
		// Pull the operator's IAM credentials whenever the session
		// changes so subsequent signed AWS calls use the new
		// principal. Drop them on sign-out so the next request falls
		// back to the admin key.
		if (this.signedIn) {
			void credentials.refresh();
		} else {
			credentials.clear();
		}
	}

	async signOut(): Promise<void> {
		await apiLogout();
		this.session = null;
		credentials.clear();
	}

	get loaded(): boolean {
		return this.session !== undefined;
	}

	get signedIn(): boolean {
		return !!this.session;
	}

	/** True when the app should redirect to /login: auth on, not signed in, bootstrap done. */
	get blocked(): boolean {
		return this.authRequired && !this.setupRequired && !this.signedIn;
	}

	get displayName(): string {
		if (!this.session) return "";
		const principal = this.session.principal;
		const slash = principal.lastIndexOf("/");
		return slash >= 0 ? principal.slice(slash + 1) : principal;
	}
}

export const auth = new AuthStore();
