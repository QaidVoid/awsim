<script lang="ts">
	import type { TableSummary } from '$lib/api/dynamodb';
	import { cn } from '$lib/utils';
	import Search from '@lucide/svelte/icons/search';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Table2 from '@lucide/svelte/icons/table-2';

	interface Props {
		tables: TableSummary[];
		selectedName: string | null;
		loading: boolean;
		onSelect: (table: TableSummary) => void;
		filter: string;
	}

	let {
		tables,
		selectedName,
		loading,
		onSelect,
		filter = $bindable('')
	}: Props = $props();

	let visible = $derived(
		filter.trim().length === 0
			? tables
			: tables.filter((t) => t.name.toLowerCase().includes(filter.trim().toLowerCase()))
	);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="border-b border-border/60 bg-background/40 p-2">
		<div class="relative">
			<Search
				class="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground"
			/>
			<input
				type="text"
				bind:value={filter}
				placeholder="Filter tables..."
				class="h-8 w-full rounded-md border border-border bg-background pr-2 pl-7 text-xs outline-none focus:border-ring focus:ring-1 focus:ring-ring"
			/>
		</div>
		<div class="mt-1.5 px-1 text-[10px] tracking-wide text-muted-foreground uppercase">
			{visible.length} of {tables.length}
		</div>
	</div>

	<div class="min-h-0 flex-1 overflow-y-auto">
		{#if loading && tables.length === 0}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if visible.length === 0}
			<div class="px-3 py-6 text-center text-xs text-muted-foreground">
				{filter ? 'No matches.' : 'No tables yet.'}
			</div>
		{:else}
			<ul class="flex flex-col">
				{#each visible as table (table.name)}
					<li>
						<button
							type="button"
							onclick={() => onSelect(table)}
							class={cn(
								'flex w-full items-start gap-2 border-b border-border/30 px-3 py-2 text-left transition-colors',
								selectedName === table.name ? 'bg-muted' : 'hover:bg-muted/50'
							)}
						>
							<Table2 class="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
							<span class="truncate font-mono text-sm">{table.name}</span>
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>
