<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import LayersIcon from '@lucide/svelte/icons/layers';
	import { toast } from 'svelte-sonner';
	import { listCachePolicies, type CachePolicy } from '$lib/api/cloudfront';

	let policies = $state<CachePolicy[]>([]);
	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			policies = await listCachePolicies();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load cache policies');
			policies = [];
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		void load();
	});
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Cache policies
			<span class="ml-1 font-normal text-muted-foreground">({policies.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		rows={policies}
		{loading}
		rowKey={(p) => p.id}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'type', label: 'Type', width: '120px', cell: typeCell },
			{ key: 'minTtl', label: 'Min TTL', width: '110px', align: 'right' },
			{ key: 'defaultTtl', label: 'Default TTL', width: '120px', align: 'right' },
			{ key: 'maxTtl', label: 'Max TTL', width: '110px', align: 'right' },
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={LayersIcon}
				title="No cache policies"
				description="Cache policies define what CloudFront caches and the cache key."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet typeCell(p: CachePolicy)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] uppercase">{p.type || 'custom'}</Badge>
{/snippet}
