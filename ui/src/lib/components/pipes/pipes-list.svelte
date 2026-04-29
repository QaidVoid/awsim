<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import CableIcon from '@lucide/svelte/icons/cable';
	import { toast } from 'svelte-sonner';
	import { listPipes, type PipeSummary, type PipeState } from '$lib/api/pipes';

	interface Props {
		onSelect: (p: PipeSummary) => void;
		onCreate: () => void;
		refreshKey?: number;
	}

	let { onSelect, onCreate, refreshKey = 0 }: Props = $props();

	let rows = $state<PipeSummary[]>([]);
	let loading = $state(false);

	$effect(() => {
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			rows = await listPipes();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load pipes');
		} finally {
			loading = false;
		}
	}

	function shortenArn(arn: string): string {
		// Trim "arn:aws:<service>:<region>:<account>:" so the table stays readable.
		const parts = arn.split(':');
		if (parts.length >= 6) return parts.slice(5).join(':');
		return arn;
	}

	function stateColor(s: PipeState): string {
		if (s === 'RUNNING') return 'text-green-500';
		if (s === 'STOPPED') return 'text-muted-foreground';
		if (s === 'CREATE_FAILED') return 'text-destructive';
		return 'text-amber-500';
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Pipes
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={onCreate}>
				<PlusIcon />
				New pipe
			</Button>
		</div>
	</div>

	<DataTable
		{rows}
		{loading}
		onRowClick={onSelect}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'source', label: 'Source', mono: true, cell: sourceCell },
			{ key: 'target', label: 'Target', mono: true, cell: targetCell },
			{ key: 'currentState', label: 'State', width: '110px', cell: stateCell }
		]}
		rowKey={(r) => r.arn}
	>
		{#snippet empty()}
			<EmptyState
				icon={CableIcon}
				title="No pipes"
				description="Pipes connect a source (SQS, Kinesis, DDB streams) to a target (Lambda, Step Functions, SQS, SNS) with optional filters and enrichment."
			>
				{#snippet action()}
					<Button onclick={onCreate}>
						<PlusIcon />
						Create pipe
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

{#snippet sourceCell(row: PipeSummary)}
	<span class="font-mono text-xs">{shortenArn(row.source)}</span>
{/snippet}

{#snippet targetCell(row: PipeSummary)}
	<span class="font-mono text-xs">{shortenArn(row.target)}</span>
{/snippet}

{#snippet stateCell(row: PipeSummary)}
	<Badge variant="outline" class={`h-5 px-2 text-[10px] ${stateColor(row.currentState)}`}>
		{row.currentState}
	</Badge>
{/snippet}
