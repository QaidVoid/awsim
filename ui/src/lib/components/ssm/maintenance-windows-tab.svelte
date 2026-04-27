<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import CalendarIcon from '@lucide/svelte/icons/calendar';
	import { toast } from 'svelte-sonner';
	import { describeMaintenanceWindows, type MaintenanceWindow } from '$lib/api/ssm';

	let windows = $state<MaintenanceWindow[]>([]);
	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			windows = await describeMaintenanceWindows();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load maintenance windows');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
	});
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">Maintenance windows ({windows.length})</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	{#snippet enabledCell(w: MaintenanceWindow)}
		{#if w.enabled}
			<Badge variant="outline" class="h-4 px-1.5 text-[10px] text-green-500">enabled</Badge>
		{:else}
			<Badge variant="outline" class="h-4 px-1.5 text-[10px] text-muted-foreground">
				disabled
			</Badge>
		{/if}
	{/snippet}

	{#snippet durationCell(w: MaintenanceWindow)}
		<span class="font-mono text-xs text-muted-foreground">
			{w.duration ?? 0}h / cutoff {w.cutoff ?? 0}h
		</span>
	{/snippet}

	{#snippet scheduleCell(w: MaintenanceWindow)}
		<span class="font-mono text-xs">{w.schedule ?? '—'}</span>
	{/snippet}

	<DataTable
		rows={windows}
		{loading}
		rowKey={(w) => w.windowId}
		columns={[
			{ key: 'windowId', label: 'Window ID', mono: true, width: '180px' },
			{ key: 'name', label: 'Name' },
			{ key: 'schedule', label: 'Schedule', cell: scheduleCell },
			{ key: 'duration', label: 'Duration / cutoff', width: '170px', cell: durationCell },
			{ key: 'enabled', label: 'State', width: '110px', cell: enabledCell },
			{ key: 'nextExecutionTime', label: 'Next', width: '210px' }
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={CalendarIcon}
				title="No maintenance windows"
				description="Schedule patch and automation runs against managed instances."
			/>
		{/snippet}
	</DataTable>
</div>
