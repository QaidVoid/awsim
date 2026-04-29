<script lang="ts">
	/**
	 * Alarms tab — table of CloudWatch metric alarms with state badges,
	 * threshold/comparison summaries, the latest evaluation reason, and a
	 * 5-second polling refresh so transitions to OK / ALARM are visible
	 * live as PutMetricData calls land.
	 */
	import { onDestroy, onMount } from 'svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { describeAlarms, type Alarm } from '$lib/api/cloudwatch-metrics';
	import { EmptyState } from '$lib/components/service';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import BellRing from '@lucide/svelte/icons/bell-ring';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Pause from '@lucide/svelte/icons/pause';
	import Play from '@lucide/svelte/icons/play';
	import { toast } from 'svelte-sonner';

	let alarms = $state<Alarm[]>([]);
	let loading = $state(true);
	let autoRefresh = $state(true);
	let timer: ReturnType<typeof setInterval> | null = null;

	function stateClass(state: string): string {
		switch (state) {
			case 'OK':
				return 'border-emerald-500/40 bg-emerald-500/15 text-emerald-400';
			case 'ALARM':
				return 'border-rose-500/40 bg-rose-500/15 text-rose-400';
			case 'INSUFFICIENT_DATA':
				return 'border-amber-500/40 bg-amber-500/15 text-amber-400';
			default:
				return 'border-border bg-muted text-muted-foreground';
		}
	}

	function compactComparison(op: string): string {
		switch (op) {
			case 'GreaterThanOrEqualToThreshold':
				return '>=';
			case 'GreaterThanThreshold':
				return '>';
			case 'LessThanThreshold':
				return '<';
			case 'LessThanOrEqualToThreshold':
				return '<=';
			default:
				return op;
		}
	}

	function relativeAge(ts: string | undefined): string {
		if (!ts) return '—';
		const updated = Date.parse(ts);
		if (Number.isNaN(updated)) return ts;
		const sec = Math.max(0, Math.round((Date.now() - updated) / 1000));
		if (sec < 5) return 'just now';
		if (sec < 60) return `${sec}s ago`;
		if (sec < 3600) return `${Math.floor(sec / 60)}m ago`;
		return `${Math.floor(sec / 3600)}h ago`;
	}

	async function reload(showSpinner = true) {
		if (showSpinner) loading = true;
		try {
			const data = await describeAlarms();
			alarms = data.alarms;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load alarms');
		} finally {
			loading = false;
		}
	}

	function startPolling() {
		if (timer) return;
		// 5 seconds: snappy enough to see PutMetricData drive a transition,
		// slow enough not to hammer the server with DescribeAlarms.
		timer = setInterval(() => reload(false), 5000);
	}

	function stopPolling() {
		if (timer) {
			clearInterval(timer);
			timer = null;
		}
	}

	function toggleAuto() {
		autoRefresh = !autoRefresh;
		if (autoRefresh) startPolling();
		else stopPolling();
	}

	onMount(() => {
		reload();
		if (autoRefresh) startPolling();
	});
	onDestroy(stopPolling);
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{alarms.length} alarm{alarms.length === 1 ? '' : 's'}
			{#if autoRefresh}
				<span class="ml-2 text-[10px] uppercase tracking-wide text-emerald-500/80">live</span>
			{/if}
		</div>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="icon-sm" onclick={toggleAuto} title={autoRefresh ? 'Pause auto-refresh' : 'Resume auto-refresh'}>
				{#if autoRefresh}<Pause class="size-3.5" />{:else}<Play class="size-3.5" />{/if}
			</Button>
			<Button variant="outline" size="sm" onclick={() => reload()} disabled={loading}>
				<RefreshCw class={loading ? 'animate-spin size-3.5' : 'size-3.5'} />
				Refresh
			</Button>
		</div>
	</header>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if loading && alarms.length === 0}
			<div class="space-y-2 p-4">
				{#each Array(4) as _, i (i)}
					<Skeleton class="h-9 w-full" />
				{/each}
			</div>
		{:else if !loading && alarms.length === 0}
			<div class="p-6">
				<EmptyState
					icon={BellRing}
					title="No alarms"
					description="Create one via: aws --endpoint-url http://localhost:4566 cloudwatch put-metric-alarm …"
				/>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Alarm</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Metric</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">State</th>
						<th class="px-4 py-2 text-right font-medium text-muted-foreground">Condition</th>
						<th class="px-4 py-2 text-right font-medium text-muted-foreground">Updated</th>
					</tr>
				</thead>
				<tbody>
					{#each alarms as a (a.alarmName)}
						<tr class="border-b border-border/40 align-top hover:bg-muted/30">
							<td class="px-4 py-2">
								<div class="font-mono text-foreground">{a.alarmName}</div>
								{#if a.alarmDescription}
									<div class="text-[11px] text-muted-foreground">{a.alarmDescription}</div>
								{/if}
								{#if a.dimensions.length}
									<div class="mt-1 flex flex-wrap gap-1">
										{#each a.dimensions as d (d.name)}
											<span class="rounded bg-muted px-1.5 py-px font-mono text-[10px] text-muted-foreground">
												{d.name}={d.value}
											</span>
										{/each}
									</div>
								{/if}
							</td>
							<td class="px-4 py-2 text-xs">
								<span class="text-muted-foreground">{a.namespace}</span>
								<span class="text-muted-foreground/60"> / </span>
								<span class="font-mono">{a.metricName}</span>
							</td>
							<td class="px-4 py-2">
								<Badge variant="outline" class={`font-mono text-[10px] ${stateClass(a.stateValue)}`}>
									{a.stateValue}
								</Badge>
								{#if a.stateReason}
									<div class="mt-1 max-w-xs text-[11px] text-muted-foreground">{a.stateReason}</div>
								{/if}
							</td>
							<td class="px-4 py-2 text-right font-mono text-xs text-muted-foreground">
								{a.statistic ?? '—'}
								{compactComparison(a.comparisonOperator)}
								{a.threshold}
								<div class="text-[10px] text-muted-foreground/70">{a.period}s × {a.evaluationPeriods}</div>
							</td>
							<td class="px-4 py-2 text-right text-xs text-muted-foreground">
								{relativeAge(a.stateUpdatedTimestamp)}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>
