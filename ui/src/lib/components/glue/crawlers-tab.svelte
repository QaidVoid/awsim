<script lang="ts">
	import { onMount } from 'svelte';
	import { getCrawlers, type GlueCrawler } from '$lib/api/glue';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import BugIcon from '@lucide/svelte/icons/bug';
	import { toast } from 'svelte-sonner';

	interface Props {
		onSelect?: (c: GlueCrawler) => void;
	}

	let { onSelect }: Props = $props();

	let rows = $state<GlueCrawler[]>([]);
	let loading = $state(true);

	onMount(load);

	async function load() {
		loading = true;
		try {
			rows = await getCrawlers();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load crawlers');
		} finally {
			loading = false;
		}
	}

	function stateVariant(s: string): 'secondary' | 'destructive' | 'outline' {
		if (s === 'READY') return 'secondary';
		if (s === 'STOPPING' || s === 'RUNNING') return 'outline';
		return 'outline';
	}
</script>

{#snippet stateCell(row: GlueCrawler)}
	<Badge variant={stateVariant(row.state)} class="h-4 px-1 text-[10px]">
		{row.state || '—'}
	</Badge>
{/snippet}

{#snippet targetsCell(row: GlueCrawler)}
	<div class="flex flex-wrap gap-1">
		{#each row.targets as t, i (i)}
			<Badge variant="outline" class="h-4 max-w-48 truncate px-1 text-[10px]">
				{t.type}: {t.path}
			</Badge>
		{:else}
			<span class="text-[10px] text-muted-foreground">—</span>
		{/each}
	</div>
{/snippet}

{#snippet roleCell(row: GlueCrawler)}
	<span class="font-mono text-[10px]">{row.role}</span>
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="text-xs text-muted-foreground">
			{rows.length} crawler{rows.length === 1 ? '' : 's'}
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
			{ key: 'state', label: 'State', cell: stateCell },
			{ key: 'databaseName', label: 'Database' },
			{ key: 'targets', label: 'Targets', cell: targetsCell },
			{ key: 'role', label: 'Role', cell: roleCell },
		]}
		rowKey={(r) => r.name}
		onRowClick={onSelect}
	>
		{#snippet empty()}
			<EmptyState
				icon={BugIcon}
				title="No crawlers"
				description="Crawlers infer schema from data sources and populate the Data Catalog."
			/>
		{/snippet}
	</DataTable>
</div>
