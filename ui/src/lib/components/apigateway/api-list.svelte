<script lang="ts">
	import type { RestApi } from '$lib/api/apigateway';
	import { cn } from '$lib/utils';
	import Search from '@lucide/svelte/icons/search';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		apis: RestApi[];
		selectedId: string | null;
		loading: boolean;
		onSelect: (api: RestApi) => void;
	}

	let { apis, selectedId, loading, onSelect }: Props = $props();

	let filter = $state('');
	let visible = $derived(
		filter.trim().length === 0
			? apis
			: apis.filter((a) =>
					(a.name + ' ' + a.id).toLowerCase().includes(filter.trim().toLowerCase())
				)
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

<div class="flex h-full min-h-0 flex-col border-r border-border">
	<div class="border-b border-border/60 bg-background/40 p-2">
		<div class="relative">
			<Search
				class="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground"
			/>
			<input
				type="text"
				bind:value={filter}
				placeholder="Filter APIs..."
				aria-label="Filter REST APIs"
				class="h-8 w-full rounded-md border border-border bg-background pr-2 pl-7 text-xs outline-none focus:border-ring focus:ring-1 focus:ring-ring"
			/>
		</div>
		<div class="mt-1.5 px-1 text-[10px] tracking-wide text-muted-foreground uppercase">
			{visible.length} of {apis.length}
		</div>
	</div>

	<div class="min-h-0 flex-1 overflow-y-auto">
		{#if loading && apis.length === 0}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if visible.length === 0}
			<div class="px-3 py-6 text-center text-xs text-muted-foreground">
				{filter ? 'No matches.' : 'No REST APIs.'}
			</div>
		{:else}
			<ul class="flex flex-col">
				{#each visible as api (api.id)}
					<li>
						<button
							type="button"
							onclick={() => onSelect(api)}
							class={cn(
								'flex w-full flex-col items-start gap-0.5 border-b border-border/30 px-3 py-2 text-left transition-colors',
								selectedId === api.id ? 'bg-muted' : 'hover:bg-muted/50'
							)}
						>
							<span class="w-full truncate text-sm font-medium">{api.name || api.id}</span>
							<span class="font-mono text-[11px] text-muted-foreground">{api.id}</span>
							<span class="text-[10px] text-muted-foreground/70">
								{shortDate(api.createdDate)}{#if api.endpointTypes.length}
									· {api.endpointTypes.join(', ')}
								{/if}
							</span>
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>
