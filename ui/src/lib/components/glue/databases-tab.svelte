<script lang="ts">
	import { onMount } from 'svelte';
	import { getDatabases, type GlueDatabase } from '$lib/api/glue';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import DatabaseIcon from '@lucide/svelte/icons/database';
	import { toast } from 'svelte-sonner';

	interface Props {
		onSelect?: (db: GlueDatabase) => void;
	}

	let { onSelect }: Props = $props();

	let rows = $state<GlueDatabase[]>([]);
	let loading = $state(true);

	onMount(load);

	async function load() {
		loading = true;
		try {
			rows = await getDatabases();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load databases');
		} finally {
			loading = false;
		}
	}
</script>

{#snippet descCell(row: GlueDatabase)}
	<span class="line-clamp-1 text-xs text-muted-foreground">{row.description || '—'}</span>
{/snippet}

{#snippet locCell(row: GlueDatabase)}
	<span class="font-mono text-[11px]">{row.locationUri || '—'}</span>
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="text-xs text-muted-foreground">
			{rows.length} database{rows.length === 1 ? '' : 's'}
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
			{ key: 'description', label: 'Description', cell: descCell },
			{ key: 'locationUri', label: 'Location', cell: locCell },
			{ key: 'createTime', label: 'Created' },
		]}
		rowKey={(r) => r.name}
		onRowClick={onSelect}
	>
		{#snippet empty()}
			<EmptyState
				icon={DatabaseIcon}
				title="No Glue databases"
				description="The Glue Data Catalog organizes tables into logical databases."
			/>
		{/snippet}
	</DataTable>
</div>
