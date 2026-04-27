<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import InboxIcon from '@lucide/svelte/icons/inbox';
	import SearchIcon from '@lucide/svelte/icons/search';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { cn } from '$lib/utils';
	import type { Queue, QueueAttributes } from '$lib/api/sqs';

	interface Props {
		queues: Queue[];
		selectedUrl: string | null;
		attrsByUrl: Record<string, QueueAttributes>;
		onSelect: (url: string) => void;
		onCreate: () => void;
	}

	let { queues, selectedUrl, attrsByUrl, onSelect, onCreate }: Props = $props();

	let filter = $state('');

	let filtered = $derived(
		filter.trim() === ''
			? queues
			: queues.filter((q) => q.name.toLowerCase().includes(filter.trim().toLowerCase()))
	);
</script>

<div class="flex h-full min-h-0 flex-col border-r border-border">
	<div class="flex items-center gap-2 border-b border-border px-3 py-2">
		<div class="relative flex-1">
			<SearchIcon
				class="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground"
			/>
			<Input
				type="search"
				placeholder="Filter queues"
				bind:value={filter}
				class="h-8 pl-7 text-xs"
			/>
		</div>
		<Button size="icon-sm" variant="outline" onclick={onCreate} aria-label="Create queue">
			<PlusIcon />
		</Button>
	</div>

	<div class="min-h-0 flex-1 overflow-y-auto">
		{#each filtered as queue (queue.url)}
			{@const attrs = attrsByUrl[queue.url]}
			{@const isSelected = selectedUrl === queue.url}
			<button
				type="button"
				class={cn(
					'block w-full border-b border-border/40 px-3 py-2 text-left transition-colors',
					isSelected ? 'bg-muted' : 'hover:bg-muted/50'
				)}
				onclick={() => onSelect(queue.url)}
			>
				<div class="flex items-center gap-2">
					<span class="truncate font-mono text-xs font-medium text-foreground">{queue.name}</span>
					{#if attrs?.isFifo}
						<Badge variant="outline" class="h-4 px-1.5 text-[10px]">FIFO</Badge>
					{/if}
				</div>
				{#if attrs}
					<div class="mt-0.5 flex items-center gap-2 text-[11px] text-muted-foreground">
						<span class="inline-flex items-center gap-1">
							<InboxIcon class="size-3" />
							{attrs.approximateNumberOfMessages}
						</span>
						{#if attrs.approximateNumberOfMessagesNotVisible > 0}
							<span class="text-amber-500">
								{attrs.approximateNumberOfMessagesNotVisible} in flight
							</span>
						{/if}
						{#if attrs.redrivePolicy}
							<span class="text-blue-500">DLQ</span>
						{/if}
					</div>
				{/if}
			</button>
		{:else}
			<div class="px-3 py-8 text-center text-xs text-muted-foreground">
				{filter ? 'No matches.' : 'No queues.'}
			</div>
		{/each}
	</div>
</div>
