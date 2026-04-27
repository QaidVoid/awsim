<script lang="ts">
	import { previewNextFireTimes } from '$lib/api/scheduler';

	interface Props {
		expression: string;
		count?: number;
	}

	let { expression, count = 5 }: Props = $props();

	let previews = $derived.by<Date[] | null>(() => {
		if (!expression.trim()) return null;
		return previewNextFireTimes(expression, count);
	});

	function fmt(d: Date): string {
		return d.toUTCString();
	}
</script>

<div class="rounded-md border border-border bg-muted/30 px-3 py-2 text-xs">
	<p class="mb-1 font-semibold text-muted-foreground">Next fire times (UTC)</p>
	{#if !expression.trim()}
		<p class="text-muted-foreground">Enter an expression to preview.</p>
	{:else if previews === null}
		<p class="text-muted-foreground">
			Preview not available for this expression. Supports
			<code>rate(N minutes|hours|days)</code> and basic
			<code>cron(min hour dom month dow)</code> with
			<code>*</code>, integers, and <code>*/N</code>.
		</p>
	{:else if previews.length === 0}
		<p class="text-muted-foreground">No matches in the next year.</p>
	{:else}
		<ul class="space-y-0.5 font-mono text-[11px]">
			{#each previews as p, i (i)}
				<li>{fmt(p)}</li>
			{/each}
		</ul>
	{/if}
</div>
