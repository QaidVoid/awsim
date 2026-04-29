<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import ActivityIcon from '@lucide/svelte/icons/activity';
	import { toast } from 'svelte-sonner';
	import { getTraceSummaries, type TraceSummary } from '$lib/api/xray';

	interface Props {
		onSelect: (trace: TraceSummary) => void;
		refreshKey?: number;
	}

	let { onSelect, refreshKey = 0 }: Props = $props();

	let rows = $state<TraceSummary[]>([]);
	let loading = $state(false);

	$effect(() => {
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			// X-Ray uses epoch seconds. Pull anything from the past hour.
			const end = Date.now() / 1000;
			const start = end - 3600;
			rows = await getTraceSummaries(start, end);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load traces');
		} finally {
			loading = false;
		}
	}

	function fmtDuration(d: number): string {
		if (d < 1) return `${(d * 1000).toFixed(0)} ms`;
		return `${d.toFixed(2)} s`;
	}

	function severityCell(row: TraceSummary): string {
		if (row.hasFault) return 'fault';
		if (row.hasError) return 'error';
		if (row.hasThrottle) return 'throttle';
		return 'ok';
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Traces in last hour
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		onRowClick={onSelect}
		columns={[
			{ key: 'id', label: 'Trace ID', mono: true },
			{ key: 'duration', label: 'Duration', width: '120px', cell: durationCell },
			{ key: 'serviceNames', label: 'Services', cell: servicesCell },
			{ key: 'hasFault', label: 'Status', width: '100px', cell: statusCell }
		]}
		rowKey={(r) => r.id}
	>
		{#snippet empty()}
			<EmptyState
				icon={ActivityIcon}
				title="No traces"
				description="Send segment documents via PutTraceSegments — the X-Ray daemon or AWS SDK X-Ray instrumentation will start populating this list."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet durationCell(row: TraceSummary)}
	<span class="font-mono text-xs">{fmtDuration(row.duration)}</span>
{/snippet}

{#snippet servicesCell(row: TraceSummary)}
	<div class="flex flex-wrap gap-1">
		{#each row.serviceNames as name (name)}
			<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">{name}</Badge>
		{/each}
	</div>
{/snippet}

{#snippet statusCell(row: TraceSummary)}
	{@const sev = severityCell(row)}
	<Badge
		variant="outline"
		class={sev === 'ok'
			? 'h-5 px-2 text-[10px] text-green-500'
			: sev === 'throttle'
				? 'h-5 px-2 text-[10px] text-amber-500'
				: 'h-5 px-2 text-[10px] text-destructive'}
	>
		{sev}
	</Badge>
{/snippet}
