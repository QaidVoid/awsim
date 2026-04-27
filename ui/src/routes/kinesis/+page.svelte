<script lang="ts">
	import { onMount } from 'svelte';
	import { ServicePage, EmptyState } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import WavesIcon from '@lucide/svelte/icons/waves';
	import { toast } from 'svelte-sonner';
	import {
		listStreams,
		describeStream,
		deleteStream,
		type Stream,
		type Shard,
	} from '$lib/api/kinesis';
	import StreamList from '$lib/components/kinesis/stream-list.svelte';
	import StreamDetailSheet from '$lib/components/kinesis/stream-detail-sheet.svelte';
	import PutRecordDialog from '$lib/components/kinesis/put-record-dialog.svelte';
	import CreateStreamDialog from '$lib/components/kinesis/create-stream-dialog.svelte';

	let streams = $state<Stream[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	let detailOpen = $state(false);
	let detailStream = $state<Stream | null>(null);
	let detailShards = $state<Shard[]>([]);

	let createOpen = $state(false);
	let putOpen = $state(false);
	let confirmDelete = $state<string | null>(null);

	async function loadStreams() {
		loading = true;
		error = null;
		try {
			const names = await listStreams();
			const detailed = await Promise.all(
				names.map(async (name) => {
					try {
						const d = await describeStream(name);
						return d.stream;
					} catch {
						return {
							name,
							status: 'UNKNOWN',
							shardCount: 0,
							retentionPeriodHours: 0,
							arn: '',
							encryptionType: 'NONE',
						} satisfies Stream;
					}
				})
			);
			streams = detailed;
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load streams';
		} finally {
			loading = false;
		}
	}

	async function openDetail(name: string) {
		try {
			const d = await describeStream(name);
			detailStream = d.stream;
			detailShards = d.shards;
			detailOpen = true;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load stream');
		}
	}

	async function handleDelete() {
		if (!confirmDelete) return;
		const name = confirmDelete;
		confirmDelete = null;
		try {
			await deleteStream(name);
			toast.success(`Stream ${name} deletion requested.`);
			await loadStreams();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}

	onMount(loadStreams);
</script>

<ServicePage
	title="Kinesis"
	description="Real-time data streams. Ingest, partition, and consume records per shard."
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={loadStreams} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<PlusIcon />
			New stream
		</Button>
	{/snippet}

	{#if error}
		<div class="px-6 py-4 text-sm text-destructive">{error}</div>
	{:else if loading && streams.length === 0}
		<div class="px-6 py-4 text-sm text-muted-foreground">Loading streams…</div>
	{:else if streams.length === 0}
		<div class="px-6 py-12">
			<EmptyState
				icon={WavesIcon}
				title="No Kinesis streams"
				description="Create a stream to publish records and consume them per shard."
			>
				{#snippet action()}
					<Button onclick={() => (createOpen = true)}>
						<PlusIcon />
						Create stream
					</Button>
				{/snippet}
			</EmptyState>
		</div>
	{:else}
		<StreamList
			{streams}
			selectedName={detailStream?.name ?? null}
			onSelect={openDetail}
			onDelete={(name) => (confirmDelete = name)}
		/>
	{/if}
</ServicePage>

<StreamDetailSheet
	open={detailOpen}
	stream={detailStream}
	shards={detailShards}
	onOpenChange={(o) => (detailOpen = o)}
	onPutRecord={() => (putOpen = true)}
/>

<PutRecordDialog
	open={putOpen}
	streamName={detailStream?.name ?? ''}
	onOpenChange={(o) => (putOpen = o)}
/>

<CreateStreamDialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={() => loadStreams()}
/>

<Dialog
	open={confirmDelete !== null}
	onOpenChange={(o) => {
		if (!o) confirmDelete = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Delete stream?</DialogTitle>
			<DialogDescription>
				This permanently removes <span class="font-mono">{confirmDelete}</span> and all of its
				records.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmDelete = null)}>Cancel</Button>
			<Button variant="destructive" onclick={handleDelete}>Delete</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
