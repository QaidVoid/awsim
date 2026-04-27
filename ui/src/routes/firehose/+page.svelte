<script lang="ts">
	import { onMount } from 'svelte';
	import { ServicePage, EmptyState, ListSkeleton } from '$lib/components/service';
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
	import FlameIcon from '@lucide/svelte/icons/flame';
	import { toast } from 'svelte-sonner';
	import {
		listDeliveryStreams,
		describeDeliveryStream,
		deleteDeliveryStream,
		type DeliveryStreamSummary,
		type DeliveryStreamDetail,
	} from '$lib/api/firehose';
	import StreamList from '$lib/components/firehose/stream-list.svelte';
	import StreamDetailSheet from '$lib/components/firehose/stream-detail-sheet.svelte';
	import PutRecordDialog from '$lib/components/firehose/put-record-dialog.svelte';
	import CreateStreamDialog from '$lib/components/firehose/create-stream-dialog.svelte';

	let streams = $state<DeliveryStreamSummary[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	let detailStream = $state<DeliveryStreamDetail | null>(null);
	let detailOpen = $state(false);

	let createOpen = $state(false);
	let putOpen = $state(false);
	let putTarget = $state<string>('');
	let confirmDelete = $state<string | null>(null);

	async function loadStreams() {
		loading = true;
		error = null;
		try {
			const names = await listDeliveryStreams();
			const detailed = await Promise.all(
				names.map(async (n) => {
					try {
						const d = await describeDeliveryStream(n);
						return {
							name: d.name,
							arn: d.arn,
							status: d.status,
							type: d.type,
							destinationType: d.destinationType,
							destinationDetail: d.destinationDetail,
							createTime: d.createTime,
							lastUpdate: d.lastUpdate,
						} satisfies DeliveryStreamSummary;
					} catch {
						return {
							name: n,
							arn: '',
							status: 'UNKNOWN',
							type: 'DirectPut',
							destinationType: 'Unknown' as const,
							destinationDetail: '—',
						} satisfies DeliveryStreamSummary;
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
			detailStream = await describeDeliveryStream(name);
			detailOpen = true;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load stream');
		}
	}

	function openPut(name: string) {
		putTarget = name;
		putOpen = true;
	}

	async function handleDelete() {
		if (!confirmDelete) return;
		const name = confirmDelete;
		confirmDelete = null;
		try {
			await deleteDeliveryStream(name);
			toast.success(`Stream ${name} deletion requested.`);
			await loadStreams();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}

	onMount(loadStreams);
</script>

<ServicePage
	title="Firehose"
	description="Buffer and deliver streaming data to S3, Redshift, OpenSearch, or HTTP endpoints."
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
		<div class="px-6 py-6">
			<ListSkeleton rows={5} />
		</div>
	{:else if streams.length === 0}
		<div class="px-6 py-12">
			<EmptyState
				icon={FlameIcon}
				title="No delivery streams"
				description="Create a stream to ingest records and write them to a destination."
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
			onPutRecord={openPut}
			onDelete={(name) => (confirmDelete = name)}
		/>
	{/if}
</ServicePage>

<StreamDetailSheet
	open={detailOpen}
	stream={detailStream}
	onOpenChange={(o) => (detailOpen = o)}
	onPutRecord={() => detailStream && openPut(detailStream.name)}
/>

<PutRecordDialog open={putOpen} streamName={putTarget} onOpenChange={(o) => (putOpen = o)} />

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
			<DialogTitle>Delete delivery stream?</DialogTitle>
			<DialogDescription>
				This permanently removes <span class="font-mono">{confirmDelete}</span>.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmDelete = null)}>Cancel</Button>
			<Button variant="destructive" onclick={handleDelete}>Delete</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
