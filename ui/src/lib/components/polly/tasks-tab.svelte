<script lang="ts">
	import { onMount } from 'svelte';
	import { listSpeechSynthesisTasks, type SpeechSynthesisTask } from '$lib/api/polly';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import ListChecksIcon from '@lucide/svelte/icons/list-checks';
	import { toast } from 'svelte-sonner';

	let rows = $state<SpeechSynthesisTask[]>([]);
	let loading = $state(true);

	onMount(load);

	async function load() {
		loading = true;
		try {
			rows = await listSpeechSynthesisTasks();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load tasks');
		} finally {
			loading = false;
		}
	}

	function statusVariant(s: string): 'secondary' | 'destructive' | 'outline' {
		if (s === 'completed') return 'secondary';
		if (s === 'failed') return 'destructive';
		return 'outline';
	}
</script>

{#snippet statusCell(row: SpeechSynthesisTask)}
	<Badge variant={statusVariant(row.taskStatus)} class="h-4 px-1 text-[10px]">
		{row.taskStatus || '—'}
	</Badge>
{/snippet}

{#snippet outputCell(row: SpeechSynthesisTask)}
	{#if row.outputUri}
		<span class="font-mono text-[10px]">{row.outputUri}</span>
	{:else}
		<span class="text-[10px] text-muted-foreground">—</span>
	{/if}
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="text-xs text-muted-foreground">
			{rows.length} task{rows.length === 1 ? '' : 's'}
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
			{ key: 'taskId', label: 'Task ID', mono: true },
			{ key: 'voiceId', label: 'Voice' },
			{ key: 'outputFormat', label: 'Format' },
			{ key: 'taskStatus', label: 'Status', cell: statusCell },
			{ key: 'outputUri', label: 'Output URI', cell: outputCell },
		]}
		rowKey={(r) => r.taskId}
	>
		{#snippet empty()}
			<EmptyState
				icon={ListChecksIcon}
				title="No synthesis tasks"
				description="Asynchronous synthesis tasks longer than the synchronous API will appear here."
			/>
		{/snippet}
	</DataTable>
</div>
