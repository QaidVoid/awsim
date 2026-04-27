<script lang="ts">
	/**
	 * Metrics tab — lists every metric known to the local emulator and
	 * inlines a sparkline of the trailing-hour Average for each row.
	 *
	 * Sparkline data is fetched lazily once per metric and cached in a
	 * map keyed by `namespace::metricName`.
	 */
	import { onMount } from 'svelte';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import {
		listMetrics,
		getMetricStatistics,
		type Metric,
	} from '$lib/api/cloudwatch-metrics';
	import { EmptyState } from '$lib/components/service';
	import BarChart from '@lucide/svelte/icons/bar-chart-3';
	import Sparkline from './sparkline.svelte';
	import { toast } from 'svelte-sonner';

	let metrics = $state<Metric[]>([]);
	let loading = $state(true);
	let query = $state('');
	let series = $state<Map<string, number[]>>(new Map());

	const filtered = $derived.by(() => {
		const q = query.trim().toLowerCase();
		if (!q) return metrics;
		return metrics.filter(
			(m) =>
				m.metricName.toLowerCase().includes(q) || m.namespace.toLowerCase().includes(q),
		);
	});

	function key(m: Metric): string {
		return `${m.namespace}::${m.metricName}::${m.dimensions.map((d) => d.name + '=' + d.value).join(',')}`;
	}

	async function loadSeries(m: Metric) {
		const k = key(m);
		if (series.has(k)) return;
		const now = Math.floor(Date.now() / 1000);
		try {
			const data = await getMetricStatistics(
				m.namespace,
				m.metricName,
				now - 3600,
				now,
				60,
				'Average',
				m.dimensions,
			);
			const points = data.datapoints.map((p) => p.average ?? 0);
			const next = new Map(series);
			next.set(k, points);
			series = next;
		} catch {
			const next = new Map(series);
			next.set(k, []);
			series = next;
		}
	}

	$effect(() => {
		const slice = filtered.slice(0, 30);
		for (const m of slice) {
			if (!series.has(key(m))) {
				loadSeries(m);
			}
		}
	});

	async function reload() {
		loading = true;
		try {
			const data = await listMetrics();
			metrics = data.metrics;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load metrics');
		} finally {
			loading = false;
		}
	}

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex shrink-0 items-center gap-2 border-b border-border px-4 py-2">
		<Input
			bind:value={query}
			placeholder="filter by namespace or metric name…"
			class="h-8 max-w-sm text-xs"
			aria-label="Filter metrics"
		/>
		<Badge variant="outline" class="ml-auto text-[11px]">
			{filtered.length} / {metrics.length}
		</Badge>
	</div>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if !loading && metrics.length === 0}
			<div class="p-6">
				<EmptyState
					icon={BarChart}
					title="No metrics"
					description="Push some with: aws --endpoint-url http://localhost:4566 cloudwatch put-metric-data --namespace MyApp --metric-name X --value 1"
				/>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead
					class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm"
				>
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Namespace</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Metric</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Dimensions</th>
						<th class="px-4 py-2 text-right font-medium text-muted-foreground">Last hour</th>
					</tr>
				</thead>
				<tbody>
					{#each filtered as m (key(m))}
						{@const data = series.get(key(m)) ?? []}
						<tr class="border-b border-border/40 hover:bg-muted/30">
							<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">
								{m.namespace}
							</td>
							<td class="px-4 py-2 font-mono text-foreground">{m.metricName}</td>
							<td class="px-4 py-2">
								{#if m.dimensions.length === 0}
									<span class="text-xs text-muted-foreground">—</span>
								{:else}
									<div class="flex flex-wrap gap-1">
										{#each m.dimensions as d (d.name + d.value)}
											<Badge variant="outline" class="font-mono text-[10px]">
												{d.name}={d.value}
											</Badge>
										{/each}
									</div>
								{/if}
							</td>
							<td class="px-4 py-2 text-right">
								{#if data.length > 0}
									<div class="flex items-center justify-end text-emerald-400">
										<Sparkline {data} ariaLabel={`${m.metricName} trend`} />
									</div>
								{:else}
									<span class="text-[10px] text-muted-foreground">—</span>
								{/if}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>
