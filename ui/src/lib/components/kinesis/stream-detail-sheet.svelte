<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription,
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import SendIcon from '@lucide/svelte/icons/send';
	import ShardList from './shard-list.svelte';
	import RecordExplorer from './record-explorer.svelte';
	import type { Stream, Shard } from '$lib/api/kinesis';

	interface Props {
		open: boolean;
		stream: Stream | null;
		shards: Shard[];
		onOpenChange: (open: boolean) => void;
		onPutRecord: () => void;
	}

	let { open, stream, shards, onOpenChange, onPutRecord }: Props = $props();

	let selectedShardId = $state<string | null>(null);

	$effect(() => {
		// reset selection when stream changes
		stream;
		selectedShardId = shards[0]?.shardId ?? null;
	});
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-3xl">
		<SheetHeader>
			<SheetTitle>
				{#if stream}<span class="font-mono">{stream.name}</span>{:else}Stream{/if}
			</SheetTitle>
			<SheetDescription>
				{#if stream}
					<span class="font-mono text-xs">{stream.arn}</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		{#if stream}
			<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4">
				<section class="rounded-md border border-border bg-card/40 p-3">
					<div class="grid grid-cols-2 gap-3 text-xs sm:grid-cols-4">
						<div>
							<dt class="text-muted-foreground">Status</dt>
							<dd>
								<Badge variant="outline" class="h-4 px-1.5 text-[10px]">{stream.status}</Badge>
							</dd>
						</div>
						<div>
							<dt class="text-muted-foreground">Shards</dt>
							<dd class="font-medium">{stream.shardCount}</dd>
						</div>
						<div>
							<dt class="text-muted-foreground">Retention</dt>
							<dd>{stream.retentionPeriodHours}h</dd>
						</div>
						<div>
							<dt class="text-muted-foreground">Encryption</dt>
							<dd>{stream.encryptionType}</dd>
						</div>
					</div>
					<div class="mt-3 flex justify-end">
						<Button size="sm" onclick={onPutRecord}>
							<SendIcon />
							Put record
						</Button>
					</div>
				</section>

				<section>
					<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">Shards</h3>
					<ShardList
						{shards}
						{selectedShardId}
						onSelect={(id) => (selectedShardId = id)}
					/>
				</section>

				{#if selectedShardId}
					<section>
						<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">
							Records · {selectedShardId}
						</h3>
						<RecordExplorer streamName={stream.name} shardId={selectedShardId} />
					</section>
				{/if}
			</div>
		{/if}
	</SheetContent>
</Sheet>
