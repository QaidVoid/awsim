<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import SearchIcon from '@lucide/svelte/icons/search';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { cn } from '$lib/utils';
	import type { Topic } from '$lib/api/sns';

	interface Props {
		topics: Topic[];
		selectedArn: string | null;
		onSelect: (arn: string) => void;
		onCreate: () => void;
	}

	let { topics, selectedArn, onSelect, onCreate }: Props = $props();

	let filter = $state('');
	let filtered = $derived(
		filter.trim() === ''
			? topics
			: topics.filter((t) => t.name.toLowerCase().includes(filter.trim().toLowerCase()))
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
				placeholder="Filter topics"
				bind:value={filter}
				class="h-8 pl-7 text-xs"
			/>
		</div>
		<Button size="icon-sm" variant="outline" onclick={onCreate} aria-label="Create topic">
			<PlusIcon />
		</Button>
	</div>

	<div class="min-h-0 flex-1 overflow-y-auto">
		{#each filtered as topic (topic.arn)}
			{@const isSelected = selectedArn === topic.arn}
			{@const fifo = topic.name.endsWith('.fifo')}
			<button
				type="button"
				class={cn(
					'block w-full border-b border-border/40 px-3 py-2 text-left transition-colors',
					isSelected ? 'bg-muted' : 'hover:bg-muted/50'
				)}
				onclick={() => onSelect(topic.arn)}
			>
				<div class="flex items-center gap-2">
					<span class="truncate font-mono text-xs font-medium">{topic.name}</span>
					{#if fifo}
						<Badge variant="outline" class="h-4 px-1.5 text-[10px]">FIFO</Badge>
					{/if}
				</div>
				<p class="mt-0.5 truncate font-mono text-[10px] text-muted-foreground">
					{topic.arn}
				</p>
			</button>
		{:else}
			<div class="px-3 py-8 text-center text-xs text-muted-foreground">
				{filter ? 'No matches.' : 'No topics.'}
			</div>
		{/each}
	</div>
</div>
