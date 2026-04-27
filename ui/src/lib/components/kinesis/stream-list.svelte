<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import WavesIcon from '@lucide/svelte/icons/waves';
	import { cn } from '$lib/utils';
	import type { Stream } from '$lib/api/kinesis';

	interface Props {
		streams: Stream[];
		selectedName: string | null;
		onSelect: (name: string) => void;
		onDelete: (name: string) => void;
	}

	let { streams, selectedName, onSelect, onDelete }: Props = $props();

	function statusClass(status: string): string {
		if (status === 'ACTIVE') return 'text-green-500';
		if (status === 'DELETING') return 'text-destructive';
		return 'text-muted-foreground';
	}
</script>

{#if streams.length === 0}
	<div class="p-6">
		<EmptyState
			icon={WavesIcon}
			title="No Kinesis streams"
			description="Create a stream to publish records and consume them per shard."
		/>
	</div>
{:else}
	<div class="overflow-hidden">
		<table class="w-full text-sm">
			<thead
				class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm"
			>
				<tr class="text-left text-xs text-muted-foreground">
					<th class="px-4 py-2 font-medium">Name</th>
					<th class="px-4 py-2 font-medium">Status</th>
					<th class="px-4 py-2 text-right font-medium">Shards</th>
					<th class="px-4 py-2 text-right font-medium">Retention</th>
					<th class="px-4 py-2 font-medium">Encryption</th>
					<th class="px-4 py-2 text-right font-medium"></th>
				</tr>
			</thead>
			<tbody>
				{#each streams as stream (stream.name)}
					{@const isSelected = selectedName === stream.name}
					<tr
						class={cn(
							'cursor-pointer border-b border-border/40 transition-colors',
							isSelected ? 'bg-muted' : 'hover:bg-muted/40'
						)}
						onclick={() => onSelect(stream.name)}
					>
						<td class="px-4 py-2 font-mono text-xs">{stream.name}</td>
						<td class="px-4 py-2 text-xs">
							<Badge variant="outline" class={cn('h-4 px-1.5 text-[10px]', statusClass(stream.status))}>
								{stream.status}
							</Badge>
						</td>
						<td class="px-4 py-2 text-right text-xs">{stream.shardCount}</td>
						<td class="px-4 py-2 text-right text-xs">{stream.retentionPeriodHours}h</td>
						<td class="px-4 py-2 text-xs text-muted-foreground">
							{stream.encryptionType}
						</td>
						<td class="px-4 py-2 text-right">
							<Button
								size="icon-xs"
								variant="ghost"
								class="text-destructive hover:text-destructive"
								onclick={(e) => {
									e.stopPropagation();
									onDelete(stream.name);
								}}
								aria-label={`Delete ${stream.name}`}
							>
								<Trash2Icon />
							</Button>
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
{/if}
