<script lang="ts">
	/**
	 * Full-viewport request log stream — a power-user variant of the
	 * dashboard's compact request stream that consumes the same shared
	 * `dashboardState` SSE buffer.
	 *
	 * Adds:
	 *   - Free-text filter (matches against service / operation / path).
	 *   - Errors (4xx+5xx) and Slow (>200ms) tabs alongside All.
	 *   - Per-column visibility toggles persisted in the parent.
	 *   - Higher capacity (caps shown rows at 500 instead of 50).
	 */
	import { Badge } from '$lib/components/ui/badge';
	import { fly } from 'svelte/transition';
	import { quintOut } from 'svelte/easing';
	import { dashboardState } from '$lib/dashboard-state.svelte';
	import { relativeTime } from '$lib/format';
	import type { RequestEvent } from '$lib/events';
	import RequestDetailSheet from '$lib/components/dashboard/request-detail-sheet.svelte';
	import type { LogTab, ColumnKey } from './types';

	interface Props {
		tab: LogTab;
		query: string;
		visibleColumns: Record<ColumnKey, boolean>;
	}

	let { tab, query, visibleColumns }: Props = $props();

	let selected = $state<RequestEvent | null>(null);
	let sheetOpen = $state(false);
	let scrollEl = $state<HTMLDivElement | null>(null);
	let pinnedToTop = $state(true);

	let now = $state(Date.now() / 1000);
	$effect(() => {
		const id = setInterval(() => (now = Date.now() / 1000), 1000);
		return () => clearInterval(id);
	});

	const filtered = $derived.by(() => {
		const all = dashboardState.events;
		const q = query.trim().toLowerCase();
		const subset = all.filter((e) => {
			if (tab === 'errors' && e.status_code < 400) return false;
			if (tab === 'slow' && e.duration_ms <= 200) return false;
			if (q.length > 0) {
				const hay = `${e.service} ${e.operation ?? ''} ${e.path}`.toLowerCase();
				if (!hay.includes(q)) return false;
			}
			return true;
		});
		return subset.slice(0, 500);
	});

	$effect(() => {
		const _firstId = filtered[0]?.id;
		void _firstId;
		if (pinnedToTop && scrollEl) {
			scrollEl.scrollTop = 0;
		}
	});

	function onScroll() {
		if (!scrollEl) return;
		pinnedToTop = scrollEl.scrollTop < 16;
	}

	function methodVariant(method: string): string {
		const m = method.toUpperCase();
		if (m === 'GET') return 'bg-sky-500/15 text-sky-400 border-sky-500/30';
		if (m === 'POST') return 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30';
		if (m === 'PUT' || m === 'PATCH')
			return 'bg-amber-500/15 text-amber-400 border-amber-500/30';
		if (m === 'DELETE') return 'bg-rose-500/15 text-rose-400 border-rose-500/30';
		return 'bg-muted text-muted-foreground border-border';
	}

	function statusClass(code: number): string {
		if (code >= 500) return 'text-rose-400';
		if (code >= 400) return 'text-amber-400';
		if (code >= 200) return 'text-emerald-400';
		return 'text-muted-foreground';
	}

	function durationClass(ms: number): string {
		if (ms >= 200) return 'text-rose-400';
		if (ms >= 50) return 'text-amber-400';
		return 'text-emerald-400';
	}

	function openDetail(evt: RequestEvent) {
		selected = evt;
		sheetOpen = true;
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div bind:this={scrollEl} onscroll={onScroll} class="min-h-0 flex-1 overflow-auto">
		{#if filtered.length === 0}
			<div class="flex h-full min-h-[200px] items-center justify-center text-xs text-muted-foreground">
				{#if dashboardState.connectionStatus === 'connecting'}
					Connecting to live stream…
				{:else if dashboardState.connectionStatus === 'closed'}
					Stream offline. Reconnecting…
				{:else if dashboardState.events.length === 0}
					No requests yet. Hit any endpoint to see activity here.
				{:else}
					No requests match the current filter.
				{/if}
			</div>
		{:else}
			<table class="w-full text-xs">
				<thead
					class="sticky top-0 z-10 bg-background/95 text-[10px] font-medium uppercase tracking-wider text-muted-foreground backdrop-blur"
				>
					<tr class="border-b border-border">
						{#if visibleColumns.time}
							<th class="px-3 py-2 text-left font-medium">Time</th>
						{/if}
						{#if visibleColumns.method}
							<th class="px-3 py-2 text-left font-medium">Method</th>
						{/if}
						{#if visibleColumns.service}
							<th class="px-3 py-2 text-left font-medium">Service</th>
						{/if}
						{#if visibleColumns.operation}
							<th class="px-3 py-2 text-left font-medium">Operation</th>
						{/if}
						{#if visibleColumns.path}
							<th class="px-3 py-2 text-left font-medium">Path</th>
						{/if}
						{#if visibleColumns.region}
							<th class="px-3 py-2 text-left font-medium">Region</th>
						{/if}
						{#if visibleColumns.status}
							<th class="px-3 py-2 text-right font-medium">Status</th>
						{/if}
						{#if visibleColumns.duration}
							<th class="px-3 py-2 text-right font-medium">Dur</th>
						{/if}
					</tr>
				</thead>
				<tbody>
					{#each filtered as evt (evt.id)}
						<tr
							in:fly={{ y: -6, duration: 180, easing: quintOut }}
							class="cursor-pointer border-b border-border/40 transition-colors hover:bg-muted/40"
							onclick={() => openDetail(evt)}
						>
							{#if visibleColumns.time}
								<td class="whitespace-nowrap px-3 py-1.5 text-muted-foreground">
									{relativeTime(evt.ts, now)}
								</td>
							{/if}
							{#if visibleColumns.method}
								<td class="px-3 py-1.5">
									<Badge variant="outline" class={`font-mono ${methodVariant(evt.method)}`}>
										{evt.method}
									</Badge>
								</td>
							{/if}
							{#if visibleColumns.service}
								<td class="px-3 py-1.5">
									<span class="font-mono text-[11px] text-foreground/80">{evt.service}</span>
								</td>
							{/if}
							{#if visibleColumns.operation}
								<td class="max-w-[220px] truncate px-3 py-1.5 font-mono text-[11px] text-foreground/80">
									{evt.operation ?? '—'}
								</td>
							{/if}
							{#if visibleColumns.path}
								<td class="max-w-[280px] truncate px-3 py-1.5 font-mono text-[11px] text-muted-foreground">
									{evt.path}
								</td>
							{/if}
							{#if visibleColumns.region}
								<td class="px-3 py-1.5 font-mono text-[11px] text-muted-foreground">
									{evt.region}
								</td>
							{/if}
							{#if visibleColumns.status}
								<td class={`px-3 py-1.5 text-right font-mono ${statusClass(evt.status_code)}`}>
									{evt.status_code}
								</td>
							{/if}
							{#if visibleColumns.duration}
								<td class={`px-3 py-1.5 text-right font-mono ${durationClass(evt.duration_ms)}`}>
									{evt.duration_ms.toFixed(0)}ms
								</td>
							{/if}
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>

<RequestDetailSheet open={sheetOpen} event={selected} onOpenChange={(o) => (sheetOpen = o)} />
