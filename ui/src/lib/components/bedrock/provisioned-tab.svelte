<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listProvisionedModelThroughputs,
		type ProvisionedModelThroughput,
	} from '$lib/api/bedrock';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import GaugeIcon from '@lucide/svelte/icons/gauge';
	import { toast } from 'svelte-sonner';

	let rows = $state<ProvisionedModelThroughput[]>([]);
	let loading = $state(true);

	onMount(load);

	async function load() {
		loading = true;
		try {
			rows = await listProvisionedModelThroughputs();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load provisioned models');
		} finally {
			loading = false;
		}
	}

	function statusVariant(s: string): 'secondary' | 'destructive' | 'outline' {
		if (s === 'InService') return 'secondary';
		if (s === 'Failed') return 'destructive';
		return 'outline';
	}
</script>

{#snippet statusCell(row: ProvisionedModelThroughput)}
	<Badge variant={statusVariant(row.status)} class="text-[10px]">{row.status || '—'}</Badge>
{/snippet}

{#snippet unitsCell(row: ProvisionedModelThroughput)}
	<span class="font-mono text-xs">
		{row.modelUnits} / <span class="text-muted-foreground">{row.desiredModelUnits}</span>
	</span>
{/snippet}

{#snippet commitmentCell(row: ProvisionedModelThroughput)}
	{#if row.commitmentDuration}
		<Badge variant="outline" class="text-[10px]">{row.commitmentDuration}</Badge>
	{:else}
		<span class="text-[10px] text-muted-foreground">—</span>
	{/if}
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="text-xs text-muted-foreground">
			{rows.length} provisioned throughput{rows.length === 1 ? '' : 's'}
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
			{ key: 'provisionedModelName', label: 'Name' },
			{ key: 'modelArn', label: 'Model ARN', mono: true },
			{ key: 'status', label: 'Status', cell: statusCell },
			{ key: 'units', label: 'Units (current/desired)', cell: unitsCell },
			{ key: 'commitment', label: 'Commitment', cell: commitmentCell },
		]}
		rowKey={(r) => r.provisionedModelArn}
	>
		{#snippet empty()}
			<EmptyState
				icon={GaugeIcon}
				title="No provisioned throughput"
				description="Reserve dedicated capacity for predictable inference performance."
			/>
		{/snippet}
	</DataTable>
</div>
