<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import CableIcon from '@lucide/svelte/icons/cable';
	import {
		listEventSourceMappings,
		deleteEventSourceMapping,
		updateEventSourceMapping,
		type EventSourceMappingSummary
	} from '$lib/api/lambda';
	import CreateEventSourceDialog from './create-event-source-dialog.svelte';

	interface Props {
		functionName: string;
	}

	let { functionName }: Props = $props();

	let rows = $state<EventSourceMappingSummary[]>([]);
	let loading = $state(false);
	let createOpen = $state(false);
	let deleteTarget = $state<EventSourceMappingSummary | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	onMount(load);

	$effect(() => {
		functionName;
		void load();
	});

	async function load() {
		loading = true;
		try {
			rows = await listEventSourceMappings(functionName);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load event sources');
		} finally {
			loading = false;
		}
	}

	async function toggleState(row: EventSourceMappingSummary) {
		try {
			const enabled = row.state !== 'Enabled';
			await updateEventSourceMapping(row.uuid, { enabled });
			toast.success(`${enabled ? 'Enabled' : 'Disabled'} mapping`);
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to update');
		}
	}

	function remove(row: EventSourceMappingSummary) {
		deleteTarget = row;
		deleteOpen = true;
	}

	async function confirmRemove() {
		const row = deleteTarget;
		if (!row) return;
		deleteBusy = true;
		try {
			await deleteEventSourceMapping(row.uuid);
			toast.success('Mapping deleted');
			deleteOpen = false;
			deleteTarget = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		} finally {
			deleteBusy = false;
		}
	}

	function shortenArn(arn: string): string {
		const parts = arn.split(':');
		if (parts.length >= 6) return parts.slice(5).join(':');
		return arn;
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="flex flex-col">
			<h3 class="text-sm font-semibold">
				Event sources
				<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
			</h3>
			<p class="text-[11px] text-muted-foreground">
				Trigger this function from SQS, Kinesis, or DynamoDB streams.
			</p>
		</div>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={() => (createOpen = true)}>
				<PlusIcon />
				Add source
			</Button>
		</div>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'eventSourceArn', label: 'Source', mono: true, cell: sourceCell },
			{ key: 'batchSize', label: 'Batch', width: '80px' },
			{ key: 'state', label: 'State', width: '120px', cell: stateCell },
			{
				key: 'lastProcessingResult',
				label: 'Last result',
				cell: lastResultCell
			},
			{ key: 'uuid', label: '', width: '90px', cell: actionsCell }
		]}
		rowKey={(r) => r.uuid}
	>
		{#snippet empty()}
			<EmptyState
				icon={CableIcon}
				title="No event sources"
				description="Hook this function up to an SQS queue, Kinesis stream, or DynamoDB stream so it gets invoked automatically."
			>
				{#snippet action()}
					<Button onclick={() => (createOpen = true)}>
						<PlusIcon />
						Add event source
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

{#snippet sourceCell(row: EventSourceMappingSummary)}
	<div class="flex flex-col gap-0.5">
		<span class="font-mono text-xs">{shortenArn(row.eventSourceArn)}</span>
		{#if row.filterCriteria?.Filters && row.filterCriteria.Filters.length > 0}
			<span class="font-mono text-[10px] text-muted-foreground">
				filter: {row.filterCriteria.Filters[0].Pattern.slice(0, 80)}
				{row.filterCriteria.Filters[0].Pattern.length > 80 ? '…' : ''}
			</span>
		{/if}
		{#if row.destinationOnFailure}
			<span class="font-mono text-[10px] text-muted-foreground">
				DLQ: {shortenArn(row.destinationOnFailure)}
			</span>
		{/if}
	</div>
{/snippet}

{#snippet stateCell(row: EventSourceMappingSummary)}
	<button
		type="button"
		onclick={() => toggleState(row)}
		title="Click to toggle"
		class="cursor-pointer"
	>
		<Badge
			variant="outline"
			class={row.state === 'Enabled'
				? 'h-5 px-2 text-[10px] text-green-500'
				: 'h-5 px-2 text-[10px] text-muted-foreground'}
		>
			{row.state}
		</Badge>
	</button>
{/snippet}

{#snippet lastResultCell(row: EventSourceMappingSummary)}
	<span
		class={row.lastProcessingResult.startsWith('PROBLEM')
			? 'text-xs text-destructive'
			: 'text-xs text-muted-foreground'}
	>
		{row.lastProcessingResult || '—'}
	</span>
{/snippet}

{#snippet actionsCell(row: EventSourceMappingSummary)}
	<Button variant="ghost" size="xs" onclick={() => remove(row)} title="Delete">
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}

<CreateEventSourceDialog
	open={createOpen}
	{functionName}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={load}
/>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete event source mapping?"
	description={`Delete event source mapping ${deleteTarget?.uuid ?? ''}.`}
	busy={deleteBusy}
	onConfirm={confirmRemove}
	onClose={() => (deleteOpen = false)}
/>
