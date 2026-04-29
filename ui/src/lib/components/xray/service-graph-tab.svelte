<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import NetworkIcon from '@lucide/svelte/icons/network';
	import { toast } from 'svelte-sonner';
	import { getServiceGraph, type ServiceGraphNode } from '$lib/api/xray';

	interface Props {
		refreshKey?: number;
	}

	let { refreshKey = 0 }: Props = $props();

	let rows = $state<ServiceGraphNode[]>([]);
	let loading = $state(false);

	$effect(() => {
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			const end = Date.now() / 1000;
			const start = end - 3600;
			rows = await getServiceGraph(start, end);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load service graph');
		} finally {
			loading = false;
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Services
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
		columns={[
			{ key: 'name', label: 'Service', mono: true },
			{ key: 'state', label: 'State', width: '90px', cell: stateCell },
			{ key: 'totalCount', label: 'Total', width: '80px' },
			{ key: 'okCount', label: 'OK', width: '80px' },
			{ key: 'errorTotalCount', label: 'Errors', width: '80px', cell: errCell },
			{ key: 'faultTotalCount', label: 'Faults', width: '80px', cell: faultCell }
		]}
		rowKey={(r) => `${r.referenceId}|${r.name}`}
	>
		{#snippet empty()}
			<EmptyState
				icon={NetworkIcon}
				title="No services in graph"
				description="Aggregate of every service name observed in trace segments over the last hour."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet stateCell(row: ServiceGraphNode)}
	<Badge variant="outline" class="h-5 px-2 text-[10px]">{row.state}</Badge>
{/snippet}

{#snippet errCell(row: ServiceGraphNode)}
	<span class={row.errorTotalCount > 0 ? 'text-destructive' : 'text-muted-foreground'}>
		{row.errorTotalCount}
	</span>
{/snippet}

{#snippet faultCell(row: ServiceGraphNode)}
	<span class={row.faultTotalCount > 0 ? 'text-destructive' : 'text-muted-foreground'}>
		{row.faultTotalCount}
	</span>
{/snippet}
