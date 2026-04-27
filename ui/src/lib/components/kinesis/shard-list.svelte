<script lang="ts">
	import { Badge } from '$lib/components/ui/badge';
	import type { Shard } from '$lib/api/kinesis';

	interface Props {
		shards: Shard[];
		selectedShardId: string | null;
		onSelect: (shardId: string) => void;
	}

	let { shards, selectedShardId, onSelect }: Props = $props();
</script>

<ul class="flex flex-col gap-1">
	{#each shards as shard (shard.shardId)}
		<li>
			<button
				type="button"
				class="flex w-full items-center justify-between gap-2 rounded-md border border-border bg-card/40 px-3 py-2 text-left text-xs transition-colors hover:bg-muted/40 aria-pressed:border-primary"
				aria-pressed={selectedShardId === shard.shardId}
				onclick={() => onSelect(shard.shardId)}
			>
				<span class="font-mono">{shard.shardId}</span>
				{#if shard.endingSequenceNumber}
					<Badge variant="outline" class="h-4 px-1.5 text-[10px]">closed</Badge>
				{:else}
					<Badge variant="outline" class="h-4 px-1.5 text-[10px] text-green-500">open</Badge>
				{/if}
			</button>
		</li>
	{:else}
		<li class="px-3 py-2 text-xs text-muted-foreground">No shards.</li>
	{/each}
</ul>
