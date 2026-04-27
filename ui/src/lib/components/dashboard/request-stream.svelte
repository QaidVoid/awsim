<script lang="ts">
	/**
	 * Live request stream — tabbed table fed by `dashboardState.events`.
	 *
	 * Behaviour:
	 *   - Caps visible rows at 50 (the underlying ring buffer holds more
	 *     for KPIs / insights).
	 *   - Auto-scrolls to top when new events arrive *unless* the user
	 *     has scrolled away from the top.
	 *   - `Pause` toggles ingestion via the shared dashboard state.
	 *   - Click a row to open the detail Sheet.
	 */
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Tabs, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import Pause from '@lucide/svelte/icons/pause';
	import Play from '@lucide/svelte/icons/play';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Activity from '@lucide/svelte/icons/activity';
	import { dashboardState } from '$lib/dashboard-state.svelte';
	import { relativeTime } from '$lib/format';
	import type { RequestEvent } from '$lib/events';
	import RequestDetailSheet from './request-detail-sheet.svelte';

	type Tab = 'all' | '4xx' | '5xx';
	let tab = $state<Tab>('all');
	let selected = $state<RequestEvent | null>(null);
	let sheetOpen = $state(false);
	let scrollEl = $state<HTMLDivElement | null>(null);
	let pinnedToTop = $state(true);

	// Tick clock so relative timestamps re-render every second.
	let now = $state(Date.now() / 1000);
	$effect(() => {
		const id = setInterval(() => (now = Date.now() / 1000), 1000);
		return () => clearInterval(id);
	});

	const filtered = $derived.by(() => {
		const all = dashboardState.events;
		const subset =
			tab === 'all'
				? all
				: tab === '4xx'
					? all.filter((e) => e.status_code >= 400 && e.status_code < 500)
					: all.filter((e) => e.status_code >= 500);
		return subset.slice(0, 50);
	});

	// Auto-scroll on new events when pinned to top.
	$effect(() => {
		// Re-run whenever the first event id changes (new arrival).
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

	function methodVariant(method: string): { class: string } {
		const m = method.toUpperCase();
		if (m === 'GET') return { class: 'bg-sky-500/15 text-sky-400 border-sky-500/30' };
		if (m === 'POST') return { class: 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30' };
		if (m === 'PUT' || m === 'PATCH') return { class: 'bg-amber-500/15 text-amber-400 border-amber-500/30' };
		if (m === 'DELETE') return { class: 'bg-rose-500/15 text-rose-400 border-rose-500/30' };
		return { class: 'bg-muted text-muted-foreground border-border' };
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

<Card class="h-full gap-0 p-0">
	<CardHeader class="flex-row items-center justify-between gap-2 border-b border-border px-4 py-3">
		<CardTitle class="flex items-center gap-2 text-sm font-semibold">
			<Activity class="size-4 text-muted-foreground" />
			Live requests
			<span class="ml-1 inline-flex items-center gap-1 text-[10px] font-normal text-muted-foreground">
				<span
					class="size-1.5 rounded-full"
					class:bg-emerald-400={dashboardState.connectionStatus === 'open'}
					class:bg-amber-400={dashboardState.connectionStatus === 'paused' || dashboardState.connectionStatus === 'connecting'}
					class:bg-muted-foreground={dashboardState.connectionStatus === 'closed'}
				></span>
				{dashboardState.connectionStatus}
			</span>
		</CardTitle>
		<div class="flex items-center gap-2">
			<Tabs value={tab} onValueChange={(v) => (tab = v as Tab)}>
				<TabsList class="h-7">
					<TabsTrigger value="all" class="h-6 px-2 text-xs">All</TabsTrigger>
					<TabsTrigger value="4xx" class="h-6 px-2 text-xs">4xx</TabsTrigger>
					<TabsTrigger value="5xx" class="h-6 px-2 text-xs">5xx</TabsTrigger>
				</TabsList>
			</Tabs>
			<Button
				size="sm"
				variant="ghost"
				class="h-7 gap-1 px-2"
				onclick={() => dashboardState.togglePause()}
				title={dashboardState.paused ? 'Resume' : 'Pause'}
			>
				{#if dashboardState.paused}
					<Play class="size-3.5" />
				{:else}
					<Pause class="size-3.5" />
				{/if}
			</Button>
			<Button
				size="sm"
				variant="ghost"
				class="h-7 gap-1 px-2"
				onclick={() => dashboardState.clear()}
				title="Clear"
			>
				<Trash2 class="size-3.5" />
			</Button>
		</div>
	</CardHeader>
	<CardContent class="p-0">
		<div
			bind:this={scrollEl}
			onscroll={onScroll}
			class="max-h-[480px] overflow-y-auto"
		>
			{#if filtered.length === 0}
				<div class="flex h-40 items-center justify-center text-xs text-muted-foreground">
					{#if dashboardState.connectionStatus === 'connecting'}
						Connecting to live stream…
					{:else if dashboardState.connectionStatus === 'closed'}
						Stream offline. Reconnecting…
					{:else}
						No requests yet. Hit any endpoint to see activity here.
					{/if}
				</div>
			{:else}
				<table class="w-full text-xs">
					<thead
						class="sticky top-0 z-10 bg-card text-[10px] font-medium uppercase tracking-wider text-muted-foreground"
					>
						<tr class="border-b border-border">
							<th class="px-3 py-2 text-left font-medium">Time</th>
							<th class="px-3 py-2 text-left font-medium">Method</th>
							<th class="px-3 py-2 text-left font-medium">Service</th>
							<th class="px-3 py-2 text-left font-medium">Operation</th>
							<th class="px-3 py-2 text-right font-medium">Status</th>
							<th class="px-3 py-2 text-right font-medium">Dur</th>
						</tr>
					</thead>
					<tbody>
						{#each filtered as evt (evt.id)}
							<tr
								class="cursor-pointer border-b border-border/40 transition-colors hover:bg-muted/40"
								onclick={() => openDetail(evt)}
							>
								<td class="whitespace-nowrap px-3 py-1.5 text-muted-foreground">
									{relativeTime(evt.ts, now)}
								</td>
								<td class="px-3 py-1.5">
									<Badge variant="outline" class={`font-mono ${methodVariant(evt.method).class}`}>
										{evt.method}
									</Badge>
								</td>
								<td class="px-3 py-1.5">
									<span class="font-mono text-[11px] text-foreground/80">{evt.service}</span>
								</td>
								<td class="max-w-[200px] truncate px-3 py-1.5 font-mono text-[11px] text-foreground/80">
									{evt.operation ?? evt.path}
								</td>
								<td class={`px-3 py-1.5 text-right font-mono ${statusClass(evt.status_code)}`}>
									{evt.status_code}
								</td>
								<td class={`px-3 py-1.5 text-right font-mono ${durationClass(evt.duration_ms)}`}>
									{evt.duration_ms.toFixed(0)}ms
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			{/if}
		</div>
	</CardContent>
</Card>

<RequestDetailSheet
	open={sheetOpen}
	event={selected}
	onOpenChange={(o) => (sheetOpen = o)}
/>
