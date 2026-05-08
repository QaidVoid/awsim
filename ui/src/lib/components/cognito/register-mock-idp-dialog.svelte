<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { registerMockIdp } from '$lib/api/mock-idp';
	import { createIdentityProvider } from '$lib/api/cognito';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		open: boolean;
		poolId: string;
		onClose: () => void;
		onCreated: () => void;
	}

	let { open = $bindable(false), poolId, onClose, onCreated }: Props = $props();

	const DEFAULT_CLAIMS = JSON.stringify(
		{
			sub: 'mock-user-001',
			email: 'user@example.com',
			email_verified: true,
			name: 'Mock User'
		},
		null,
		2
	);

	let providerName = $state('MockIdP');
	let providerId = $state('mockidp');
	let claimsJson = $state(DEFAULT_CLAIMS);
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!open) {
			providerName = 'MockIdP';
			providerId = 'mockidp';
			claimsJson = DEFAULT_CLAIMS;
			saving = false;
			error = null;
		}
	});

	async function submit() {
		const trimmedName = providerName.trim();
		const trimmedId = providerId.trim();
		if (!trimmedName) {
			error = 'Cognito provider name is required';
			return;
		}
		if (!/^[a-zA-Z0-9_]+$/.test(trimmedId)) {
			error = 'Mock provider id must be alphanumeric / underscore (it shows up in URLs).';
			return;
		}

		let parsedClaims: Record<string, unknown>;
		try {
			parsedClaims = JSON.parse(claimsJson);
			if (typeof parsedClaims !== 'object' || Array.isArray(parsedClaims)) {
				error = 'Default claims must be a JSON object';
				return;
			}
		} catch (e) {
			error = `Default claims is not valid JSON: ${e instanceof Error ? e.message : 'parse error'}`;
			return;
		}

		saving = true;
		error = null;
		try {
			// 1. Register on the awsim mock IdP side.
			const reg = await registerMockIdp({
				provider_id: trimmedId,
				default_claims: parsedClaims
			});

			// 2. Register matching Cognito-side IdentityProvider so
			//    /oauth2/authorize?identity_provider=<name> wires up
			//    immediately. The discovery URL drives endpoint
			//    resolution server-side; we still pass a starter
			//    AttributeMapping so the federated user lands with
			//    email + name populated by default.
			await createIdentityProvider({
				poolId,
				name: trimmedName,
				type: 'OIDC',
				providerDetails: {
					oidc_issuer: reg.discovery_url.replace(/\/\.well-known\/openid-configuration$/, ''),
					client_id: reg.client_id,
					client_secret: reg.client_secret,
					authorize_scopes: 'openid email profile'
				},
				attributeMapping: {
					email: 'email',
					name: 'name'
				}
			});

			toast.success(`Registered mock IdP ${trimmedName}`);
			onCreated();
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Register failed';
			error = msg;
			toast.error(msg);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle>Add awsim mock IdP</DialogTitle>
			<DialogDescription>
				Spins up a built-in OIDC provider at <code>/_awsim/idp/&lt;id&gt;</code> and registers a
				matching Cognito IdentityProvider on this pool, ready for offline federation testing. No
				external network calls.
			</DialogDescription>
		</DialogHeader>
		<form
			class="flex flex-col gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			<div class="grid gap-3 sm:grid-cols-2">
				<div class="space-y-1.5">
					<Label for="cog-name">Cognito provider name</Label>
					<Input id="cog-name" bind:value={providerName} autocomplete="off" />
					<p class="text-[11px] text-muted-foreground">
						The string you pass as <code>?identity_provider=</code> on
						<code>/oauth2/authorize</code>.
					</p>
				</div>
				<div class="space-y-1.5">
					<Label for="mock-id">Mock IdP id</Label>
					<Input id="mock-id" bind:value={providerId} autocomplete="off" />
					<p class="text-[11px] text-muted-foreground">
						URL slug for the mock endpoints (alphanumeric / underscore).
					</p>
				</div>
			</div>

			<div class="space-y-1.5">
				<Label for="claims">
					Default claims
					<span class="text-muted-foreground">(pre-fills the IdP login form)</span>
				</Label>
				<textarea
					id="claims"
					bind:value={claimsJson}
					class="min-h-[180px] w-full rounded-md border border-input bg-transparent p-2 font-mono text-xs"
				></textarea>
				<p class="text-[11px] text-muted-foreground">
					Anything you put here is what the mock IdP issues on sign-in. The Cognito-side
					AttributeMapping translates these claims to user attributes.
				</p>
			</div>

			{#if error}
				<p class="text-xs text-destructive">{error}</p>
			{/if}
			<DialogFooter>
				<Button type="button" variant="outline" onclick={onClose} disabled={saving}>
					Cancel
				</Button>
				<Button type="submit" disabled={saving || !providerName.trim() || !providerId.trim()}>
					{#if saving}<Loader2 class="size-3.5 animate-spin" />{/if}
					Register
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
