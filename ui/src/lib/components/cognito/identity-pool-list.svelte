<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { route } from '$lib/url';
	import { listIdentityPools, type IdentityPool } from '$lib/api/cognito';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import Fingerprint from '@lucide/svelte/icons/fingerprint';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';

	let pools = $state<IdentityPool[]>([]);
	let loading = $state(false);
	let filter = $state('');

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

	function open(p: IdentityPool) {
		void goto(route(`/cognito/identity/${encodeURIComponent(p.id)}`));
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
