<script lang="ts">
	import type { Bucket } from '$lib/api/s3';
	import { cn } from '$lib/utils';
	import Search from '@lucide/svelte/icons/search';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Database from '@lucide/svelte/icons/database';

	interface Props {
		buckets: Bucket[];
		selectedName: string | null;
		loading: boolean;
		onSelect: (bucket: Bucket) => void;
		filter: string;
	}

	let { buckets, selectedName, loading, onSelect, filter = $bindable('') }: Props = $props();

	let visible = $derived(
		filter.trim().length === 0
			? buckets
			: buckets.filter((b) => b.name.toLowerCase().includes(filter.trim().toLowerCase()))
	);

	function shortDate(iso: string): string {
		if (!iso) return '—';
		try {
			return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
		} catch {
			return iso;
		}
	}
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
				placeholder="Filter buckets..."
				class="h-8 w-full rounded-md border border-border bg-background pr-2 pl-7 text-xs outline-none focus:border-ring focus:ring-1 focus:ring-ring"
			/>
		</div>
		<div class="mt-1.5 px-1 text-[10px] tracking-wide text-muted-foreground uppercase">
			{visible.length} of {buckets.length}
		</div>
	</div>

	<div class="min-h-0 flex-1 overflow-y-auto">
		{#if loading && buckets.length === 0}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if visible.length === 0}
			<div class="px-3 py-6 text-center text-xs text-muted-foreground">
				{filter ? 'No matches.' : 'No buckets yet.'}
			</div>
		{:else}
			<ul class="flex flex-col">
				{#each visible as bucket (bucket.name)}
					<li>
						<button
							type="button"
							onclick={() => onSelect(bucket)}
							class={cn(
								'flex w-full items-start gap-2 border-b border-border/30 px-3 py-2 text-left transition-colors',
								selectedName === bucket.name ? 'bg-muted' : 'hover:bg-muted/50'
							)}
						>
							<Database class="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
							<div class="min-w-0 flex-1">
								<span class="block w-full truncate font-mono text-sm">{bucket.name}</span>
								<span class="text-[11px] text-muted-foreground">
									{shortDate(bucket.creationDate)}
								</span>
							</div>
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>
