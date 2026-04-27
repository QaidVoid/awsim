<script lang="ts">
	import { onMount } from 'svelte';
	import { getJobs, type GlueJob } from '$lib/api/glue';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import BriefcaseIcon from '@lucide/svelte/icons/briefcase';
	import { toast } from 'svelte-sonner';

	interface Props {
		onSelect?: (j: GlueJob) => void;
	}

	let { onSelect }: Props = $props();

	let rows = $state<GlueJob[]>([]);
	let loading = $state(true);

	onMount(load);

	async function load() {
		loading = true;
		try {
			rows = await getJobs();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load jobs');
		} finally {
			loading = false;
		}
	}
</script>

{#snippet typeCell(row: GlueJob)}
	{#if row.command.name}
		<Badge variant="outline" class="h-4 px-1 text-[10px]">{row.command.name}</Badge>
	{:else}
		<span class="text-[10px] text-muted-foreground">—</span>
	{/if}
{/snippet}

{#snippet versionCell(row: GlueJob)}
	<span class="font-mono text-xs">{row.glueVersion ?? '—'}</span>
{/snippet}

{#snippet workersCell(row: GlueJob)}
	<span class="text-xs">
		{row.workerType ?? '—'}
		{#if row.numberOfWorkers}
			<span class="text-muted-foreground"> × {row.numberOfWorkers}</span>
		{/if}
	</span>
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="text-xs text-muted-foreground">
			{rows.length} job{rows.length === 1 ? '' : 's'}
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
			{ key: 'type', label: 'Command', cell: typeCell },
			{ key: 'glueVersion', label: 'Version', cell: versionCell },
			{ key: 'workers', label: 'Workers', cell: workersCell },
			{ key: 'lastModifiedOn', label: 'Modified' },
		]}
		rowKey={(r) => r.name}
		onRowClick={onSelect}
	>
		{#snippet empty()}
			<EmptyState
				icon={BriefcaseIcon}
				title="No Glue jobs"
				description="Jobs run ETL scripts on managed Spark or Python shell environments."
			/>
		{/snippet}
	</DataTable>
</div>
