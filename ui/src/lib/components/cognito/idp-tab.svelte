<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		listIdentityProviders,
		deleteIdentityProvider,
		type IdentityProvider
	} from '$lib/api/cognito';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import CreateIdpDialog from './create-idp-dialog.svelte';
	import IdpDetail from './idp-detail.svelte';
	import ConfirmDialog from './confirm-dialog.svelte';

	interface Props {
		poolId: string;
	}

	let { poolId }: Props = $props();

	let providers = $state<IdentityProvider[]>([]);
	let loading = $state(false);
	let expanded = $state<string | null>(null);
	let createOpen = $state(false);
	let deleteName = $state<string | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	onMount(load);

	async function load() {
		loading = true;
		try {
			const r = await listIdentityProviders(poolId);
			providers = r.providers;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load providers');
		} finally {
			loading = false;
		}
	}

	function openDelete(name: string) {
		deleteName = name;
		deleteOpen = true;
	}

	async function confirmDelete() {
		if (!deleteName) return;
		deleteBusy = true;
		try {
			await deleteIdentityProvider(poolId, deleteName);
			toast.success(`Deleted ${deleteName}`);
			deleteOpen = false;
			deleteName = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}
</script>

<div class="space-y-3">
	<div class="flex items-center gap-2">
		<p class="text-xs text-muted-foreground">
			External identity sources federated via OIDC, SAML, or social providers.
		</p>
		<div class="flex-1"></div>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
		<Button size="xs" onclick={() => (createOpen = true)}>
			<Plus class="size-3.5" /> Provider
		</Button>
	</div>

	{#if loading && providers.length === 0}
		<p class="text-xs text-muted-foreground">Loading...</p>
	{:else if providers.length === 0}
		<p class="text-xs text-muted-foreground">No identity providers configured.</p>
	{:else}
		<ul class="space-y-1.5">
			{#each providers as p (p.name)}
				<li class="rounded border border-border/60">
					<div class="flex flex-wrap items-center gap-2 px-3 py-2 text-sm">
						<button
							type="button"
							class="flex min-w-0 flex-1 items-center gap-1.5 text-left"
							onclick={() => (expanded = expanded === p.name ? null : p.name)}
							aria-expanded={expanded === p.name}
							aria-label="Toggle details for {p.name}"
						>
							<ChevronRight
								class="size-3.5 shrink-0 text-muted-foreground transition-transform {expanded ===
								p.name
									? 'rotate-90'
									: ''}"
							/>
							<span class="truncate font-medium">{p.name}</span>
							<Badge variant="outline" class="font-mono text-[10px]">{p.type}</Badge>
						</button>
						<Button
							variant="ghost"
							size="xs"
							class="text-destructive hover:text-destructive"
							onclick={() => openDelete(p.name)}
						>
							Delete
						</Button>
					</div>
					{#if expanded === p.name}
						<div class="border-t border-border/60 px-3 py-3">
							{#key p.name}
								<IdpDetail {poolId} name={p.name} />
							{/key}
						</div>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</div>

<CreateIdpDialog
	bind:open={createOpen}
	{poolId}
	onClose={() => (createOpen = false)}
	onCreated={() => void load()}
/>
{#if deleteName}
	<ConfirmDialog
		bind:open={deleteOpen}
		title="Delete identity provider"
		description={`Delete provider "${deleteName}"? Federated sign-ins via this IdP will stop working.`}
		busy={deleteBusy}
		onConfirm={confirmDelete}
		onClose={() => {
			deleteOpen = false;
			deleteName = null;
		}}
	/>
{/if}
