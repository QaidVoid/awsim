<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listQueryExecutions,
		batchGetQueryExecution,
		type QueryExecution,
	} from '$lib/api/athena';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import HistoryIcon from '@lucide/svelte/icons/history';
	import { toast } from 'svelte-sonner';

	let rows = $state<QueryExecution[]>([]);
	let loading = $state(true);

	onMount(load);

	async function load() {
		loading = true;
		try {
			const ids = await listQueryExecutions();
			rows = await batchGetQueryExecution(ids.slice(0, 50));
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load history');
		} finally {
			loading = false;
		}
	}

	function statusVariant(s: string): 'secondary' | 'destructive' | 'outline' {
		if (s === 'SUCCEEDED') return 'secondary';
		if (s === 'FAILED' || s === 'CANCELLED') return 'destructive';
		return 'outline';
	}

	function shortQuery(q: string): string {
		const oneLine = q.replace(/\s+/g, ' ').trim();
		return oneLine.length > 80 ? oneLine.slice(0, 80) + '…' : oneLine;
	}
</script>

{#snippet statusCell(row: QueryExecution)}
	<Badge variant={statusVariant(row.status.state)} class="h-4 px-1 text-[10px]">
		{row.status.state || '—'}
	</Badge>
{/snippet}

{#snippet queryCell(row: QueryExecution)}
	<span class="font-mono text-[11px]">{shortQuery(row.query)}</span>
{/snippet}

{#snippet timeCell(row: QueryExecution)}
	<span class="text-[10px] text-muted-foreground">
		{row.status.submissionDateTime ?? '—'}
	</span>
{/snippet}

{#snippet statsCell(row: QueryExecution)}
	{#if row.statistics}
		<span class="text-[10px] text-muted-foreground">
			{row.statistics.engineExecutionTimeInMillis} ms
		</span>
	{:else}
		<span class="text-[10px] text-muted-foreground">—</span>
	{/if}
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="text-xs text-muted-foreground">
			{rows.length} execution{rows.length === 1 ? '' : 's'}
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
			{ key: 'queryExecutionId', label: 'ID', mono: true },
			{ key: 'query', label: 'Query', cell: queryCell },
			{ key: 'workGroup', label: 'WG' },
			{ key: 'status', label: 'Status', cell: statusCell },
			{ key: 'time', label: 'Submitted', cell: timeCell },
			{ key: 'stats', label: 'Engine ms', cell: statsCell },
		]}
		rowKey={(r) => r.queryExecutionId}
	>
		{#snippet empty()}
			<EmptyState
				icon={HistoryIcon}
				title="No query history"
				description="Run a query from the editor tab to see executions here."
			/>
		{/snippet}
	</DataTable>
</div>
