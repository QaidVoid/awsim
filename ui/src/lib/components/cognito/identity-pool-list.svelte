<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listIdentityPools,
		describeIdentityPool,
		type IdentityPool,
		type IdentityPoolDetail
	} from '$lib/api/cognito';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import Fingerprint from '@lucide/svelte/icons/fingerprint';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';

	let pools = $state<IdentityPool[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let selected = $state<IdentityPoolDetail | null>(null);
	let detailLoading = $state(false);
	let sheetOpen = $state(false);

	const filtered = $derived(
		filter.trim()
			? pools.filter((p) => p.name.toLowerCase().includes(filter.trim().toLowerCase()))
			: pools
	);

	async function load() {
		loading = true;
		try {
			pools = await listIdentityPools();
		} finally {
			loading = false;
		}
	}

	async function open(p: IdentityPool) {
		sheetOpen = true;
		detailLoading = true;
		try {
			selected = await describeIdentityPool(p.id);
		} finally {
			detailLoading = false;
		}
	}

	onMount(load);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input
			type="search"
			placeholder="Filter identity pools..."
			bind:value={filter}
			class="h-8 max-w-xs"
		/>
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {pools.length}</Badge>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
	</div>
	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
			{loading}
			columns={[
				{ key: 'name', label: 'Name', width: '40%' },
				{ key: 'id', label: 'Pool ID', mono: true, width: '40%' },
				{
					key: 'allowUnauthenticated',
					label: 'Unauth',
					width: '15%',
					cell: cellUnauth
				}
			]}
			rowKey={(r: IdentityPool) => r.id}
			onRowClick={open}
		>
			{#snippet empty()}
				<EmptyState
					icon={Fingerprint}
					title="No identity pools"
					description="Identity pools federate identities for AWS access."
				/>
			{/snippet}
		</DataTable>
	</div>
</div>

{#snippet cellUnauth(r: IdentityPool)}
	{#if r.allowUnauthenticated}
		<Badge variant="outline">enabled</Badge>
	{:else}
		<span class="text-xs text-muted-foreground">disabled</span>
	{/if}
{/snippet}

<Sheet bind:open={sheetOpen} onOpenChange={(v) => (sheetOpen = v)}>
	<SheetContent side="right" class="w-full max-w-xl overflow-y-auto sm:max-w-xl">
		<SheetHeader>
			<SheetTitle>{selected?.name ?? ''}</SheetTitle>
			<SheetDescription class="font-mono text-xs">{selected?.id ?? ''}</SheetDescription>
		</SheetHeader>
		<div class="px-6 pb-6">
			{#if detailLoading}
				<p class="text-xs text-muted-foreground">Loading...</p>
			{:else if selected}
				<dl class="grid grid-cols-3 gap-x-4 gap-y-2 py-4 text-sm">
					<dt class="text-muted-foreground">Allow unauthenticated</dt>
					<dd class="col-span-2">{selected.allowUnauthenticated ? 'Yes' : 'No'}</dd>
					{#if selected.developerProviderName}
						<dt class="text-muted-foreground">Developer provider</dt>
						<dd class="col-span-2 font-mono text-xs">{selected.developerProviderName}</dd>
					{/if}
				</dl>
				{#if selected.cognitoIdentityProviders?.length}
					<h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
						Cognito identity providers
					</h3>
					<ul class="space-y-1.5">
						{#each selected.cognitoIdentityProviders as p (p.providerName + p.clientId)}
							<li class="rounded border border-border/60 px-3 py-2 font-mono text-xs">
								<div>{p.providerName}</div>
								<div class="text-muted-foreground">client: {p.clientId}</div>
							</li>
						{/each}
					</ul>
				{/if}
			{/if}
		</div>
	</SheetContent>
</Sheet>
