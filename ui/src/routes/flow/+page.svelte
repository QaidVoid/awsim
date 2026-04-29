<script lang="ts">
	/**
	 * Activity Flow — groups the live SSE event stream into "bursts"
	 * (sequences of requests separated by no more than `gapMs` ms) so
	 * a fan-out (Lambda → DynamoDB → SQS → SNS) shows up as one
	 * collapsible card instead of four flat rows.
	 *
	 * This is heuristic, not a true distributed trace — awsim doesn't
	 * propagate parent_id through internal handler calls — but
	 * temporal proximity is a useful first-order grouping for spotting
	 * cross-service activity.
	 */
	import { onDestroy, onMount } from 'svelte';
	import { ServicePage, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import ActivityIcon from '@lucide/svelte/icons/activity';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import EyeIcon from '@lucide/svelte/icons/eye';
	import { dashboardState } from '$lib/dashboard-state.svelte';
	import { inspectState } from '$lib/inspect-state.svelte';
	import type { RequestEvent } from '$lib/events';

	let gapMs = $state(250);
	let expanded = $state<Record<number, boolean>>({});

	onMount(() => dashboardState.connect());
	onDestroy(() => dashboardState.disconnect());

	interface Burst {
		startTs: number;
		endTs: number;
		events: RequestEvent[]; // chronological (oldest → newest)
		serviceCounts: Map<string, number>;
		errorCount: number;
		slowCount: number;
	}

	let bursts = $derived.by<Burst[]>(() => {
		// dashboardState.events is newest-first; flip to chronological so
		// "consecutive within Nms" is well-defined.
		const events = [...dashboardState.events].reverse();
		if (events.length === 0) return [];
		const result: Burst[] = [];
		let current: Burst | null = null;
		for (const evt of events) {
			const ts = evt.ts * 1000;
			if (current && ts - current.endTs <= gapMs) {
				current.events.push(evt);
				current.endTs = ts;
				current.serviceCounts.set(
					evt.service,
					(current.serviceCounts.get(evt.service) ?? 0) + 1,
				);
				if (evt.status_code >= 400) current.errorCount++;
				if (evt.duration_ms > 200) current.slowCount++;
			} else {
				current = {
					startTs: ts,
					endTs: ts,
					events: [evt],
					serviceCounts: new Map([[evt.service, 1]]),
					errorCount: evt.status_code >= 400 ? 1 : 0,
					slowCount: evt.duration_ms > 200 ? 1 : 0,
				};
				result.push(current);
			}
		}
		// Newest burst first for display.
		return result.reverse();
	});

	function toggle(idx: number) {
		expanded = { ...expanded, [idx]: !expanded[idx] };
	}

	function expandAll() {
		const all: Record<number, boolean> = {};
		for (let i = 0; i < bursts.length; i++) all[i] = true;
		expanded = all;
	}

	function collapseAll() {
		expanded = {};
	}

	function formatRange(start: number, end: number): string {
		const startD = new Date(start);
		const dur = end - start;
		const time = startD.toLocaleTimeString();
		if (dur < 1) return time;
		if (dur < 1000) return `${time} +${dur.toFixed(0)}ms`;
		return `${time} +${(dur / 1000).toFixed(2)}s`;
	}

	function statusVariant(code: number): 'default' | 'destructive' | 'outline' {
		if (code >= 500) return 'destructive';
		if (code >= 400) return 'outline';
		return 'default';
	}

	function inspect(evt: RequestEvent) {
		inspectState.show(evt.id, evt);
	}

	let totalEvents = $derived(bursts.reduce((acc, b) => acc + b.events.length, 0));
	let totalErrors = $derived(bursts.reduce((acc, b) => acc + b.errorCount, 0));
</script>

<svelte:head>
	<title>AWSim · Activity Flow</title>
</svelte:head>

<ServicePage
	title="Activity flow"
	description="Live request stream grouped into bursts. Useful for spotting Lambda→Dynamo→SNS-style fan-out without hand-correlating timestamps."
>
	{#snippet actions()}
		<Badge variant="outline" class="gap-1.5">
			<span
				class="size-1.5 rounded-full"
				class:bg-emerald-400={dashboardState.connectionStatus === 'open'}
				class:bg-amber-400={dashboardState.connectionStatus === 'paused' ||
					dashboardState.connectionStatus === 'connecting'}
				class:bg-muted-foreground={dashboardState.connectionStatus === 'closed'}
			></span>
			<span class="text-[11px]">{dashboardState.connectionStatus}</span>
		</Badge>
		<Badge variant="outline" class="text-[11px]">
			{bursts.length} burst{bursts.length === 1 ? '' : 's'} · {totalEvents} req · {totalErrors} err
		</Badge>
	{/snippet}

	{#snippet toolbar()}
		<div class="flex items-center gap-3 text-xs">
			<Label for="gap" class="whitespace-nowrap">Burst gap</Label>
			<Input
				id="gap"
				type="number"
				min="50"
				max="5000"
				step="50"
				bind:value={gapMs}
				class="h-7 w-20 text-xs"
			/>
			<span class="text-muted-foreground">ms</span>
			<div class="ml-2 flex gap-1">
				<Button variant="ghost" size="sm" onclick={expandAll} class="h-7 px-2">
					Expand all
				</Button>
				<Button variant="ghost" size="sm" onclick={collapseAll} class="h-7 px-2">
					Collapse
				</Button>
			</div>
		</div>
	{/snippet}

	<div class="p-4">
		{#if bursts.length === 0}
			<EmptyState
				icon={ActivityIcon}
				title="No requests yet"
				description="Hit any AWSim endpoint — bursts will appear here as they happen."
			/>
		{:else}
			<div class="space-y-2">
				{#each bursts as burst, i (burst.events[0].id)}
					{@const isOpen = !!expanded[i]}
					<div class="rounded-md border border-border bg-card overflow-hidden">
						<button
							type="button"
							onclick={() => toggle(i)}
							class="flex w-full items-center gap-3 px-3 py-2 text-left transition-colors hover:bg-muted/40"
						>
							{#if isOpen}
								<ChevronDownIcon class="size-4 shrink-0 text-muted-foreground" />
							{:else}
								<ChevronRightIcon class="size-4 shrink-0 text-muted-foreground" />
							{/if}
							<span class="font-mono text-[11px] text-muted-foreground whitespace-nowrap">
								{formatRange(burst.startTs, burst.endTs)}
							</span>
							<div class="flex flex-1 flex-wrap items-center gap-1">
								{#each [...burst.serviceCounts.entries()].sort((a, b) => b[1] - a[1]) as [svc, count] (svc)}
									<Badge variant="secondary" class="font-mono text-[10px]">
										{svc} × {count}
									</Badge>
								{/each}
							</div>
							{#if burst.errorCount > 0}
								<Badge variant="destructive" class="text-[10px]">
									{burst.errorCount} err
								</Badge>
							{/if}
							{#if burst.slowCount > 0}
								<Badge variant="outline" class="text-[10px]">
									{burst.slowCount} slow
								</Badge>
							{/if}
							<span class="font-mono text-[10px] text-muted-foreground">
								{burst.events.length} req
							</span>
						</button>

						{#if isOpen}
							<div class="border-t border-border bg-muted/20">
								{#each burst.events as evt, ei (evt.id)}
									{@const offsetMs = evt.ts * 1000 - burst.startTs}
									<div
										class="grid grid-cols-[60px_60px_60px_1fr_auto_auto] items-center gap-2 border-t border-border/40 px-3 py-1.5 text-[11px] first:border-t-0 hover:bg-muted/40"
									>
										<span class="font-mono text-muted-foreground" title="Offset from burst start">
											+{offsetMs.toFixed(0)}ms
										</span>
										<Badge variant="outline" class="justify-self-start font-mono text-[10px]">
											{evt.method}
										</Badge>
										<Badge variant={statusVariant(evt.status_code)} class="font-mono text-[10px]">
											{evt.status_code}
										</Badge>
										<span class="truncate font-mono">
											<span class="text-muted-foreground">{evt.service}</span>
											{#if evt.operation}
												<span class="mx-1 text-muted-foreground/60">·</span>
												{evt.operation}
											{:else}
												<span class="mx-1 text-muted-foreground/60">·</span>
												<span class="truncate text-muted-foreground/70">{evt.path}</span>
											{/if}
										</span>
										<span class="font-mono text-muted-foreground" title="Request duration">
											{evt.duration_ms.toFixed(0)}ms
										</span>
										<Button
											variant="ghost"
											size="sm"
											onclick={() => inspect(evt)}
											class="h-6 px-1.5"
										>
											<EyeIcon class="size-3.5" />
										</Button>
									</div>
									{#if ei === burst.events.length - 1 && burst.errorCount > 0 && evt.error_code}
										<!-- noop, error already surfaced by status badge -->
									{/if}
								{/each}
							</div>
						{/if}
					</div>
				{/each}
			</div>
		{/if}
	</div>
</ServicePage>
