<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import { Input } from '$lib/components/ui/input';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import RewindIcon from '@lucide/svelte/icons/rewind';
	import FileSearchIcon from '@lucide/svelte/icons/file-search';
	import { toast } from 'svelte-sonner';
	import {
		getShardIterator,
		getRecords,
		decodeRecordData,
		type KinesisRecord,
		type ShardIteratorType,
	} from '$lib/api/kinesis';

	interface Props {
		streamName: string;
		shardId: string;
	}

	let { streamName, shardId }: Props = $props();

	let iteratorType = $state<ShardIteratorType>('TRIM_HORIZON');
	let iterator = $state<string | null>(null);
	let records = $state<KinesisRecord[]>([]);
	let limit = $state(20);
	let loading = $state(false);
	let millisBehind = $state<number | null>(null);

	$effect(() => {
		// Reset on shard change
		shardId;
		iterator = null;
		records = [];
		millisBehind = null;
	});

	async function refreshIterator() {
		loading = true;
		try {
			iterator = await getShardIterator(streamName, shardId, iteratorType);
			records = [];
			millisBehind = null;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to get shard iterator');
		} finally {
			loading = false;
		}
	}

	async function fetchNext() {
		if (!iterator) {
			await refreshIterator();
			if (!iterator) return;
		}
		loading = true;
		try {
			const res = await getRecords(iterator, limit);
			records = [...records, ...res.records];
			iterator = res.nextShardIterator ?? null;
			millisBehind = res.millisBehindLatest ?? null;
			if (res.records.length === 0) toast.info('No new records.');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to fetch records');
		} finally {
			loading = false;
		}
	}

	function preview(b64: string): string {
		const decoded = decodeRecordData(b64);
		return decoded.length > 200 ? decoded.slice(0, 200) + '…' : decoded;
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex flex-wrap items-end gap-3 rounded-md border border-border bg-card/40 p-3">
		<div class="flex flex-col gap-1">
			<Label for="kin-iter-type">Iterator type</Label>
			<Select
				type="single"
				value={iteratorType}
				onValueChange={(v) => (iteratorType = v as ShardIteratorType)}
			>
				<SelectTrigger id="kin-iter-type" class="w-[180px]">
					{iteratorType}
				</SelectTrigger>
				<SelectContent>
					<SelectItem value="TRIM_HORIZON" label="TRIM_HORIZON"
						>TRIM_HORIZON</SelectItem
					>
					<SelectItem value="LATEST" label="LATEST">LATEST</SelectItem>
				</SelectContent>
			</Select>
		</div>
		<div class="flex flex-col gap-1">
			<Label for="kin-limit">Limit</Label>
			<Input id="kin-limit" type="number" min="1" max="1000" bind:value={limit} class="w-24" />
		</div>
		<div class="flex-1"></div>
		<Button variant="outline" size="sm" onclick={refreshIterator} disabled={loading}>
			<RewindIcon />
			Reset iterator
		</Button>
		<Button size="sm" onclick={fetchNext} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			{loading ? 'Fetching…' : 'Fetch next'}
		</Button>
	</div>

	{#if millisBehind !== null}
		<p class="text-xs text-muted-foreground">
			Latency: {millisBehind}ms behind tip ·
			{records.length} record{records.length === 1 ? '' : 's'}
		</p>
	{/if}

	{#if records.length === 0}
		<EmptyState
			icon={FileSearchIcon}
			title="No records loaded"
			description="Pick an iterator type and click Fetch next to read records from this shard."
		/>
	{:else}
		<ul class="flex flex-col gap-2">
			{#each records as r (r.sequenceNumber)}
				<li class="rounded-md border border-border bg-card/40 p-3">
					<div class="flex items-center justify-between gap-2 text-[11px] text-muted-foreground">
						<span class="truncate font-mono">{r.sequenceNumber}</span>
						<div class="flex items-center gap-2">
							<Badge variant="outline" class="h-4 px-1.5 text-[10px]">
								{r.partitionKey}
							</Badge>
							<span>
								{new Date(r.approximateArrivalTimestamp * 1000).toLocaleTimeString()}
							</span>
						</div>
					</div>
					<pre
						class="mt-2 max-h-40 overflow-auto text-xs font-mono whitespace-pre-wrap break-all">{preview(
							r.data
						)}</pre>
				</li>
			{/each}
		</ul>
	{/if}
</div>
