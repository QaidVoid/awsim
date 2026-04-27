<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import SendIcon from '@lucide/svelte/icons/send';
	import FlameIcon from '@lucide/svelte/icons/flame';
	import { cn } from '$lib/utils';
	import type { DeliveryStreamSummary } from '$lib/api/firehose';

	interface Props {
		streams: DeliveryStreamSummary[];
		selectedName: string | null;
		onSelect: (name: string) => void;
		onPutRecord: (name: string) => void;
		onDelete: (name: string) => void;
	}

	let { streams, selectedName, onSelect, onPutRecord, onDelete }: Props = $props();

	function statusClass(status: string): string {
		if (status === 'ACTIVE') return 'text-green-500';
		if (status === 'DELETING') return 'text-destructive';
		if (status === 'CREATING') return 'text-amber-500';
		return 'text-muted-foreground';
	}
</script>

{#if streams.length === 0}
	<div class="p-6">
		<EmptyState
			icon={FlameIcon}
			title="No delivery streams"
			description="Firehose streams batch records and write them to S3, Redshift, OpenSearch, or HTTP endpoints."
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
					<th class="px-4 py-2 font-medium">Type</th>
					<th class="px-4 py-2 font-medium">Destination</th>
					<th class="px-4 py-2 font-medium">Detail</th>
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
							<Badge
								variant="outline"
								class={cn('h-4 px-1.5 text-[10px]', statusClass(stream.status))}
							>
								{stream.status}
							</Badge>
						</td>
						<td class="px-4 py-2 text-xs text-muted-foreground">{stream.type}</td>
						<td class="px-4 py-2 text-xs">
							<Badge variant="outline" class="h-4 px-1.5 text-[10px]">
								{stream.destinationType}
							</Badge>
						</td>
						<td class="truncate px-4 py-2 font-mono text-[10px] text-muted-foreground">
							{stream.destinationDetail}
						</td>
						<td class="px-4 py-2 text-right">
							<div class="flex justify-end gap-1">
								<Button
									size="icon-xs"
									variant="ghost"
									onclick={(e) => {
										e.stopPropagation();
										onPutRecord(stream.name);
									}}
									aria-label={`Put record to ${stream.name}`}
								>
									<SendIcon />
								</Button>
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
							</div>
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
{/if}
