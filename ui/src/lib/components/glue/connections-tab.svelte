<script lang="ts">
	import { onMount } from 'svelte';
	import { getConnections, type GlueConnection } from '$lib/api/glue';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlugIcon from '@lucide/svelte/icons/plug';
	import { toast } from 'svelte-sonner';

	interface Props {
		onSelect?: (c: GlueConnection) => void;
	}

	let { onSelect }: Props = $props();

	let rows = $state<GlueConnection[]>([]);
	let loading = $state(true);

	onMount(load);

	async function load() {
		loading = true;
		try {
			rows = await getConnections();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load connections');
		} finally {
			loading = false;
		}
	}
</script>

{#snippet typeCell(row: GlueConnection)}
	{#if row.connectionType}
		<Badge variant="outline" class="h-4 px-1 text-[10px]">{row.connectionType}</Badge>
	{:else}
		<span class="text-[10px] text-muted-foreground">—</span>
	{/if}
{/snippet}

{#snippet descCell(row: GlueConnection)}
	<span class="line-clamp-1 text-xs text-muted-foreground">{row.description || '—'}</span>
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="text-xs text-muted-foreground">
			{rows.length} connection{rows.length === 1 ? '' : 's'}
		</div>
		<Button variant="outline" size="sm" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name' },
			{ key: 'connectionType', label: 'Type', cell: typeCell },
			{ key: 'description', label: 'Description', cell: descCell },
			{ key: 'lastUpdatedTime', label: 'Updated' },
		]}
		rowKey={(r) => r.name}
		onRowClick={onSelect}
	>
		{#snippet empty()}
			<EmptyState
				icon={PlugIcon}
				title="No connections"
				description="Connections store credentials for crawlers and jobs to reach data stores."
			/>
		{/snippet}
	</DataTable>
</div>
