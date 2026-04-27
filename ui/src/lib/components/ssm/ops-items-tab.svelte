<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import AlertCircleIcon from '@lucide/svelte/icons/alert-circle';
	import { toast } from 'svelte-sonner';
	import { describeOpsItems, type OpsItem } from '$lib/api/ssm';

	let items = $state<OpsItem[]>([]);
	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			items = await describeOpsItems();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load ops items');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
	});

	function statusVariant(s?: string): 'green' | 'amber' | 'red' | 'muted' {
		const v = (s ?? '').toLowerCase();
		if (v === 'open') return 'amber';
		if (v === 'inprogress' || v === 'in_progress') return 'amber';
		if (v === 'resolved') return 'green';
		if (v === 'closed') return 'muted';
		return 'red';
	}

	function statusClass(s?: string): string {
		const v = statusVariant(s);
		if (v === 'green') return 'text-green-500';
		if (v === 'amber') return 'text-amber-500';
		if (v === 'red') return 'text-destructive';
		return 'text-muted-foreground';
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">OpsItems ({items.length})</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	{#snippet statusCell(o: OpsItem)}
		<Badge variant="outline" class={`h-4 px-1.5 text-[10px] ${statusClass(o.status)}`}>
			{o.status ?? '—'}
		</Badge>
	{/snippet}

	{#snippet priorityCell(o: OpsItem)}
		<span class="font-mono text-xs">{o.priority ?? '—'}</span>
	{/snippet}

	<DataTable
		rows={items}
		{loading}
		rowKey={(o) => o.opsItemId}
		columns={[
			{ key: 'opsItemId', label: 'ID', mono: true, width: '180px' },
			{ key: 'title', label: 'Title' },
			{ key: 'status', label: 'Status', width: '110px', cell: statusCell },
			{ key: 'priority', label: 'Priority', width: '90px', cell: priorityCell },
			{ key: 'severity', label: 'Severity', width: '110px' },
			{ key: 'source', label: 'Source', width: '160px' },
			{ key: 'createdTime', label: 'Created', width: '210px' }
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={AlertCircleIcon}
				title="No OpsItems"
				description="OpsItems aggregate operational issues from CloudWatch alarms, security findings, and more."
			/>
		{/snippet}
	</DataTable>
</div>
