<script lang="ts">
	/**
	 * Event History tab — recent CloudTrail events with attribute filter and
	 * a click-to-open detail sheet.
	 */
	import { onMount } from 'svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import History from '@lucide/svelte/icons/history';
	import {
		lookupEvents,
		type LookupAttributeKey,
		type TrailEvent,
	} from '$lib/api/cloudtrail';
	import { EmptyState } from '$lib/components/service';
	import EventFilterPopover from './event-filter-popover.svelte';
	import EventDetailSheet from './event-detail-sheet.svelte';
	import { toast } from 'svelte-sonner';

	let events = $state<TrailEvent[]>([]);
	let loading = $state(true);
	let attribute = $state<{ key: LookupAttributeKey; value: string } | null>(null);
	let selected = $state<TrailEvent | null>(null);
	let sheetOpen = $state(false);

	async function reload() {
		loading = true;
		try {
			const data = await lookupEvents({
				attribute: attribute ?? undefined,
				maxResults: 200,
			});
			events = data.events.sort((a, b) => b.eventTime - a.eventTime);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load events');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		// Re-fetch when the filter attribute changes.
		void attribute;
		reload();
	});

	function openDetail(e: TrailEvent) {
		selected = e;
		sheetOpen = true;
	}

	function fmtTime(ms: number): string {
		try {
			return new Date(ms).toLocaleString();
		} catch {
			return String(ms);
		}
	}

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex shrink-0 items-center gap-2 border-b border-border px-4 py-2">
		<Button size="sm" variant="ghost" class="h-8 px-2" onclick={reload} aria-label="Refresh events">
			<RefreshCw class={`size-3.5 ${loading ? 'animate-spin' : ''}`} />
		</Button>
		<EventFilterPopover {attribute} onApply={(a) => (attribute = a)} />
		<Badge variant="outline" class="ml-auto text-[11px]">{events.length} events</Badge>
	</div>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if !loading && events.length === 0}
			<div class="p-6">
				<EmptyState
					icon={History}
					title="No events"
					description={attribute
						? 'No events match the current filter.'
						: 'Hit any AWS API to populate event history.'}
				/>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead
					class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm"
				>
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Time</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Event</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Source</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">User</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Region</th>
					</tr>
				</thead>
				<tbody>
					{#each events as e (e.eventId)}
						<tr
							class="cursor-pointer border-b border-border/40 hover:bg-muted/30"
							onclick={() => openDetail(e)}
						>
							<td class="whitespace-nowrap px-4 py-2 font-mono text-[11px] text-muted-foreground">
								{fmtTime(e.eventTime)}
							</td>
							<td class="px-4 py-2 font-mono text-foreground">{e.eventName}</td>
							<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">
								{e.eventSource}
							</td>
							<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">
								{e.username ?? '—'}
							</td>
							<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">
								{e.region ?? '—'}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>

<EventDetailSheet
	open={sheetOpen}
	event={selected}
	onOpenChange={(o) => (sheetOpen = o)}
/>
