<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { getUserPoolMfaConfig, setUserPoolMfaConfig, type MfaConfig } from '$lib/api/cognito';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		poolId: string;
	}

	let { poolId }: Props = $props();

	let original = $state<MfaConfig | null>(null);
	let cfg = $state<MfaConfig>({ mfaConfiguration: 'OFF', softwareTokenEnabled: false });
	let loading = $state(true);
	let saving = $state(false);

	const dirty = $derived.by(() => {
		if (!original) return false;
		return (
			original.mfaConfiguration !== cfg.mfaConfiguration ||
			original.softwareTokenEnabled !== cfg.softwareTokenEnabled
		);
	});

	onMount(load);

	async function load() {
		loading = true;
		try {
			const m = await getUserPoolMfaConfig(poolId);
			original = m;
			cfg = { ...m };
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load MFA');
		} finally {
			loading = false;
		}
	}

	async function save() {
		saving = true;
		try {
			await setUserPoolMfaConfig(poolId, cfg);
			toast.success('MFA configuration saved');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Save failed');
		} finally {
			saving = false;
		}
	}

	function reset() {
		if (original) cfg = { ...original };
	}
</script>

<div class="space-y-4 rounded border border-border/60 px-3 py-3">
	<div>
		<h3 class="text-sm font-semibold">Multi-factor authentication</h3>
		<p class="text-xs text-muted-foreground">
			Pool-level MFA mode. Per-user enrollment is managed via AdminSetUserMFAPreference at
			sign-in.
		</p>
	</div>

	{#if loading}
		<p class="text-xs text-muted-foreground">
			<Loader2 class="inline size-3 animate-spin" /> Loading...
		</p>
	{:else}
		<div class="space-y-2">
			<Label class="text-xs uppercase tracking-wide text-muted-foreground">MFA mode</Label>
			<div class="flex gap-2">
				{#each ['OFF', 'OPTIONAL', 'ON'] as mode (mode)}
					<button
						type="button"
						class="flex-1 rounded border px-3 py-2 text-sm transition-colors {cfg.mfaConfiguration ===
						mode
							? 'border-primary bg-primary/10 text-primary'
							: 'border-border bg-background text-muted-foreground hover:border-border/80'}"
						onclick={() => (cfg.mfaConfiguration = mode as MfaConfig['mfaConfiguration'])}
					>
						<div class="font-medium">{mode}</div>
						<div class="text-[11px] text-muted-foreground">
							{#if mode === 'OFF'}
								MFA disabled for all users
							{:else if mode === 'OPTIONAL'}
								Users may opt in
							{:else}
								All users must use MFA
							{/if}
						</div>
					</button>
				{/each}
			</div>
		</div>

		<div class="space-y-2">
			<Label class="text-xs uppercase tracking-wide text-muted-foreground">Factors</Label>
			<label class="flex items-start gap-2 text-sm">
				<input
					type="checkbox"
					bind:checked={cfg.softwareTokenEnabled}
					disabled={cfg.mfaConfiguration === 'OFF'}
					class="mt-0.5 size-3.5"
				/>
				<span>
					<span class="font-medium">Authenticator apps (TOTP)</span>
					<span class="block text-xs text-muted-foreground">
						Enables AssociateSoftwareToken / VerifySoftwareToken for sign-in.
					</span>
				</span>
			</label>
		</div>

		<div class="flex justify-end gap-2">
			<Button variant="ghost" size="sm" onclick={reset} disabled={saving || !dirty}>
				Discard
			</Button>
			<Button size="sm" onclick={save} disabled={saving || !dirty}>
				{#if saving}<Loader2 class="size-3.5 animate-spin" />{/if}
				Save
			</Button>
		</div>
	{/if}
</div>
