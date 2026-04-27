<script lang="ts">
	import type { StackEvent } from '$lib/api/cloudformation';
	import { stackStatusVariant } from '$lib/api/cloudformation';
	import { Badge } from '$lib/components/ui/badge';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import History from '@lucide/svelte/icons/history';

	interface Props {
		events: StackEvent[];
		loading: boolean;
	}

	let { events, loading }: Props = $props();

	function fmt(iso: string): string {
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}

	// Sort newest first.
	let sorted = $derived(
		[...events].sort((a, b) => (a.timestamp < b.timestamp ? 1 : -1))
	);
</script>

<div class="min-h-0 flex-1 overflow-auto">
	{#if loading && events.length === 0}
		<div class="space-y-3 p-4">
			{#each Array(5) as _, i (i)}
				<Skeleton class="h-12 w-full" />
			{/each}
		</div>
	{:else if events.length === 0}
		<div class="p-6">
			<EmptyState icon={History} title="No events" description="No stack events recorded." />
		</div>
	{:else}
		<ol class="relative ml-6 mt-4 mr-4 border-l border-border pl-6">
			{#each sorted as e (e.eventId)}
				<li class="mb-4 last:mb-0">
					<span
						class="absolute -left-1.5 mt-1.5 size-3 rounded-full border border-border bg-background"
					></span>
					<div class="flex flex-col gap-1 rounded-md border border-border bg-card px-3 py-2">
						<div class="flex flex-wrap items-center gap-2">
							<Badge variant={stackStatusVariant(e.resourceStatus)} class="text-[10px]">
								{e.resourceStatus}
							</Badge>
							<span class="font-mono text-xs">{e.logicalResourceId}</span>
							<span class="font-mono text-[11px] text-muted-foreground">{e.resourceType}</span>
							<span class="ml-auto text-[11px] text-muted-foreground">{fmt(e.timestamp)}</span>
						</div>
						{#if e.resourceStatusReason}
							<p class="text-[11px] text-muted-foreground">{e.resourceStatusReason}</p>
						{/if}
					</div>
				</li>
			{/each}
		</ol>
	{/if}
</div>
