<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlayIcon from '@lucide/svelte/icons/play';
	import HistoryIcon from '@lucide/svelte/icons/history';
	import { toast } from 'svelte-sonner';
	import { listJobs, startJob, type BackupJob } from '$lib/api/backup';

	interface Props {
		refreshKey?: number;
		onChanged?: () => void;
	}

	let { refreshKey = 0, onChanged }: Props = $props();

	let rows = $state<BackupJob[]>([]);
	let loading = $state(false);
	let newVault = $state('');
	let newResource = $state('');
	let starting = $state(false);

	$effect(() => {
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			rows = await listJobs();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load jobs');
		} finally {
			loading = false;
		}
	}

	async function start() {
		if (!newVault.trim() || !newResource.trim()) {
			return toast.error('Vault name and resource ARN are required.');
		}
		starting = true;
		try {
			await startJob(newVault.trim(), newResource.trim());
			toast.success('Backup job started.');
			newResource = '';
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to start job');
		} finally {
			starting = false;
		}
	}

	function stateColor(s: string): string {
		if (s === 'COMPLETED') return 'text-green-500';
		if (s === 'FAILED' || s === 'EXPIRED' || s === 'ABORTED') return 'text-destructive';
		return 'text-amber-500';
	}

	function timestamp(t?: number): string {
		if (!t) return '—';
		return new Date(t * 1000).toLocaleString();
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Backup jobs
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="flex flex-wrap items-center gap-2">
		<Input bind:value={newVault} placeholder="vault name" class="h-8 max-w-[180px]" />
		<Input
			bind:value={newResource}
			placeholder="arn:aws:dynamodb:..."
			class="h-8 min-w-[300px] flex-1 font-mono text-xs"
		/>
		<Button size="sm" onclick={start} disabled={starting}>
			<PlayIcon />
			{starting ? 'Starting…' : 'Start backup'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'jobId', label: 'Job', mono: true },
			{ key: 'resourceType', label: 'Type', width: '100px' },
			{ key: 'resourceArn', label: 'Resource', mono: true, cell: resCell },
			{ key: 'state', label: 'State', width: '110px', cell: stateCell },
			{ key: 'percentDone', label: '%', width: '60px' },
			{ key: 'completionDate', label: 'Completed', width: '180px', cell: completedCell }
		]}
		rowKey={(r) => r.jobId}
	>
		{#snippet empty()}
			<EmptyState
				icon={HistoryIcon}
				title="No backup jobs"
				description="Start a backup job to capture a recovery point for any AWS resource."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet resCell(row: BackupJob)}
	<span class="font-mono text-xs">{row.resourceArn.split(':').slice(5).join(':')}</span>
{/snippet}

{#snippet stateCell(row: BackupJob)}
	<Badge variant="outline" class={`h-5 px-2 text-[10px] ${stateColor(row.state)}`}>
		{row.state}
	</Badge>
{/snippet}

{#snippet completedCell(row: BackupJob)}
	<span class="text-xs">{timestamp(row.completionDate)}</span>
{/snippet}
