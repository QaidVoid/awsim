<script lang="ts">
	/**
	 * Alarms tab — table of CloudWatch metric alarms with state badges and
	 * threshold/comparison summaries.
	 */
	import { onMount } from 'svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { describeAlarms, type Alarm } from '$lib/api/cloudwatch-metrics';
	import { EmptyState } from '$lib/components/service';
	import BellRing from '@lucide/svelte/icons/bell-ring';
	import { toast } from 'svelte-sonner';

	let alarms = $state<Alarm[]>([]);
	let loading = $state(true);

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

	async function reload() {
		loading = true;
		try {
			const data = await describeAlarms();
			alarms = data.alarms;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load alarms');
		} finally {
			loading = false;
		}
	}

	onMount(reload);
</script>

<div class="h-full overflow-auto">
	{#if !loading && alarms.length === 0}
		<div class="p-6">
			<EmptyState
				icon={BellRing}
				title="No alarms"
				description="Create one via: aws --endpoint-url http://localhost:4566 cloudwatch put-metric-alarm …"
			/>
		</div>
	{:else}
		<table class="w-full text-sm">
			<thead
				class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm"
			>
				<tr>
					<th class="px-4 py-2 text-left font-medium text-muted-foreground">Alarm</th>
					<th class="px-4 py-2 text-left font-medium text-muted-foreground">Metric</th>
					<th class="px-4 py-2 text-left font-medium text-muted-foreground">State</th>
					<th class="px-4 py-2 text-right font-medium text-muted-foreground">Condition</th>
					<th class="px-4 py-2 text-right font-medium text-muted-foreground">Period</th>
				</tr>
			</thead>
			<tbody>
				{#each alarms as a (a.alarmName)}
					<tr class="border-b border-border/40 hover:bg-muted/30">
						<td class="px-4 py-2">
							<div class="font-mono text-foreground">{a.alarmName}</div>
							{#if a.alarmDescription}
								<div class="text-[11px] text-muted-foreground">{a.alarmDescription}</div>
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
						</td>
						<td class="px-4 py-2 text-right font-mono text-xs text-muted-foreground">
							{a.statistic ?? '—'}
							{compactComparison(a.comparisonOperator)}
							{a.threshold}
						</td>
						<td class="px-4 py-2 text-right font-mono text-xs text-muted-foreground">
							{a.period}s × {a.evaluationPeriods}
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	{/if}
</div>
