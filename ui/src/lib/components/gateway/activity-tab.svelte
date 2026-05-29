<script lang="ts">
	// Recent gateway invocations — one row per outer call
	// (InvokeModel / Converse / Embed), with the candidate list
	// the runtime actually tried. Auto-polls every 5s while the
	// tab is visible; resets on awsim restart (in-memory ring).
	import { onMount, onDestroy } from 'svelte';
	import {
		getGatewayMetrics,
		getGatewayRecent,
		type InvocationRecord,
		type MetricTotals,
		type Outcome,
	} from '$lib/api/gateway';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import {
		Table,
		TableBody,
		TableCell,
		TableHead,
		TableHeader,
		TableRow,
	} from '$lib/components/ui/table';
	import { EmptyState } from '$lib/components/service';
	import ActivityIcon from '@lucide/svelte/icons/activity';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { toast } from 'svelte-sonner';

	const POLL_MS = 5000;

	let invocations = $state<InvocationRecord[]>([]);
	let totals = $state<MetricTotals | null>(null);
	let loading = $state(true);
	let expanded = $state<Record<string, boolean>>({});
	let timer: ReturnType<typeof setInterval> | null = null;

	onMount(async () => {
		await reload();
		timer = setInterval(reload, POLL_MS);
	});

	onDestroy(() => {
		if (timer !== null) clearInterval(timer);
	});

	async function reload() {
		try {
			const [r, m] = await Promise.all([getGatewayRecent(), getGatewayMetrics()]);
			invocations = r.invocations;
			totals = m.totals;
		} catch (e) {
			if (loading) {
				toast.error(e instanceof Error ? e.message : 'Failed to load activity');
			}
		} finally {
			loading = false;
		}
	}

	function outcomeBadge(o: Outcome): {
		variant: 'default' | 'secondary' | 'destructive' | 'outline';
		label: string;
	} {
		switch (o) {
			case 'success':
				return { variant: 'default', label: 'OK' };
			case 'retriable':
				return { variant: 'secondary', label: 'Retriable' };
			case 'fatal':
				return { variant: 'destructive', label: 'Fatal' };
		}
	}

	function relativeAgo(iso: string): string {
		const ms = Date.now() - new Date(iso).getTime();
		if (ms < 1000) return 'just now';
		if (ms < 60_000) return `${Math.floor(ms / 1000)}s ago`;
		if (ms < 3_600_000) return `${Math.floor(ms / 60_000)}m ago`;
		return `${Math.floor(ms / 3_600_000)}h ago`;
	}

	function recordKey(r: InvocationRecord): string {
		return `${r.at}-${r.bedrockId}`;
	}

	function formatCost(usd: number): string {
		// Choose precision based on magnitude so a $3.00 call doesn't
		// render as "$3.000000" and a $0.0000045 call doesn't truncate
		// to "$0.00".
		if (usd === 0) return '$0';
		if (usd >= 1) return `$${usd.toFixed(2)}`;
		if (usd >= 0.01) return `$${usd.toFixed(4)}`;
		return `$${usd.toFixed(6)}`;
	}

	function toggle(key: string) {
		expanded[key] = !expanded[key];
	}
</script>

<div class="space-y-4 p-4">
	<section class="rounded-lg border bg-card p-4 text-sm">
		<div class="flex items-start gap-3">
			<ActivityIcon class="mt-0.5 h-5 w-5 shrink-0 text-muted-foreground" />
			<div class="space-y-1">
				<p class="font-semibold">Recent invocations</p>
				<p class="text-muted-foreground">
					Last ~200 outer calls (InvokeModel / Converse / Embed) with the candidate list the runtime
					tried. Resets on awsim restart. Tap a row to see per-attempt latency + the upstream error
					when fallback kicked in.
				</p>
			</div>
		</div>
	</section>

	<header class="flex items-center justify-between">
		<div>
			<h2 class="text-lg font-semibold">Activity</h2>
			<p class="text-sm text-muted-foreground">
				{invocations.length} record{invocations.length === 1 ? '' : 's'} · auto-refresh every 5s
				{#if totals}
					· {totals.total} total attempt{totals.total === 1 ? '' : 's'}
					· <span class="text-emerald-600">{totals.success} ok</span>
					· <span class="text-amber-600">{totals.retriable} retriable</span>
					· <span class="text-rose-600">{totals.fatal} fatal</span>
					{#if totals.promptTokensTotal > 0 || totals.completionTokensTotal > 0}
						· <span class="font-mono">{totals.promptTokensTotal.toLocaleString()} in
						/ {totals.completionTokensTotal.toLocaleString()} out tokens</span>
					{/if}
					{#if totals.costUsdTotal > 0}
						· <span class="font-mono font-semibold text-foreground">{formatCost(totals.costUsdTotal)} spent</span>
					{/if}
				{/if}
			</p>
		</div>
		<Button variant="ghost" size="sm" onclick={reload} disabled={loading}>
			<RefreshCwIcon class={loading ? 'h-4 w-4 animate-spin' : 'h-4 w-4'} />
			<span class="ml-2">Refresh</span>
		</Button>
	</header>

	{#if loading && invocations.length === 0}
		<EmptyState icon={ActivityIcon} title="Loading activity…" />
	{:else if invocations.length === 0}
		<EmptyState
			icon={ActivityIcon}
			title="No invocations yet"
			description="Once an SDK or the Bedrock playground talks to InvokeModel / Converse / Embed, the candidate list and per-attempt latency show up here."
		/>
	{:else}
		<div class="rounded-lg border bg-card">
			<Table>
				<TableHeader>
					<TableRow>
						<TableHead class="w-12"></TableHead>
						<TableHead>When</TableHead>
						<TableHead>Op</TableHead>
						<TableHead>Bedrock id</TableHead>
						<TableHead>Outcome</TableHead>
						<TableHead>Attempts</TableHead>
						<TableHead>Tokens</TableHead>
						<TableHead>Cost</TableHead>
						<TableHead>Latency</TableHead>
					</TableRow>
				</TableHeader>
				<TableBody>
					{#each invocations as r (recordKey(r))}
						{@const key = recordKey(r)}
						{@const ob = outcomeBadge(r.outcome)}
						{@const isOpen = expanded[key] ?? false}
						<TableRow>
							<TableCell>
								<Button
									variant="ghost"
									size="icon"
									onclick={() => toggle(key)}
									aria-label={isOpen ? 'Collapse' : 'Expand'}
								>
									<ChevronRightIcon
										class={isOpen ? 'h-4 w-4 rotate-90 transition-transform' : 'h-4 w-4 transition-transform'}
									/>
								</Button>
							</TableCell>
							<TableCell class="text-xs text-muted-foreground" title={r.at}>
								{relativeAgo(r.at)}
							</TableCell>
							<TableCell>
								<Badge variant="outline" class="text-[10px] uppercase">{r.op}</Badge>
							</TableCell>
							<TableCell class="font-mono text-xs">{r.bedrockId}</TableCell>
							<TableCell>
								<Badge variant={ob.variant} class="text-[10px] uppercase">{ob.label}</Badge>
							</TableCell>
							<TableCell class="text-xs">{r.attempts.length}</TableCell>
							<TableCell class="font-mono text-xs text-muted-foreground">
								{#if r.promptTokens != null || r.completionTokens != null}
									{r.promptTokens ?? 0} / {r.completionTokens ?? 0}
								{:else}
									—
								{/if}
							</TableCell>
							<TableCell class="font-mono text-xs">
								{#if r.costUsd != null}
									<span class="font-semibold text-foreground" title="Cost stamped from this id's pricing override">
										{formatCost(r.costUsd)}
									</span>
								{:else}
									<span class="text-muted-foreground" title="No pricing override set for this Bedrock id">—</span>
								{/if}
							</TableCell>
							<TableCell class="font-mono text-xs">{r.totalLatencyMs}ms</TableCell>
						</TableRow>
						{#if isOpen}
							<TableRow>
								<TableCell colspan={9} class="bg-muted/30">
									{#if r.attempts.length === 0}
										<p class="py-2 text-xs text-muted-foreground">No attempts recorded.</p>
									{:else}
										<div class="flex flex-col gap-1 py-2 text-xs">
											{#each r.attempts as a, i (i)}
												{@const ab = outcomeBadge(a.outcome)}
												<div class="flex items-start gap-2">
													<span class="rounded bg-muted/40 px-1.5 py-0.5 font-mono text-[10px]">
														#{i + 1}
													</span>
													<code class="font-mono">{a.backend}</code>
													<span class="text-muted-foreground">→</span>
													<code class="font-mono">{a.tag}</code>
													<Badge variant={ab.variant} class="text-[10px] uppercase">
														{ab.label}
													</Badge>
													<span class="font-mono text-muted-foreground">{a.latencyMs}ms</span>
													{#if a.error}
														<span class="ml-auto truncate text-rose-600" title={a.error}>
															{a.error}
														</span>
													{/if}
												</div>
											{/each}
										</div>
									{/if}
								</TableCell>
							</TableRow>
						{/if}
					{/each}
				</TableBody>
			</Table>
		</div>
	{/if}
</div>
