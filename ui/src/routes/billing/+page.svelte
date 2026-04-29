<script lang="ts">
	/**
	 * Billing dashboard — shows the rolling estimated bill computed from
	 * AWSim's metered request counts × embedded AWS pricing. Auto-refreshes
	 * every 5s so users can watch the cost climb in real time.
	 */
	import { onDestroy, onMount } from 'svelte';
	import { ServicePage, EmptyState } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import DollarSignIcon from '@lucide/svelte/icons/dollar-sign';
	import TrendingUpIcon from '@lucide/svelte/icons/trending-up';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import SkullIcon from '@lucide/svelte/icons/skull';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import { fetchBilling, type BillingReport, type BillingService } from '$lib/api';
	import { toast } from 'svelte-sonner';

	const REFRESH_INTERVAL_MS = 5_000;
	const HISTORY_WINDOW_MS = 30 * 60 * 1000; // 30 minutes
	const HISTORY_KEY = 'awsim-billing-history';

	interface HistoryPoint {
		ts: number;
		running_cost_usd: number;
		projected_monthly_cost_usd: number;
	}

	// AWS publishes verbose service names ("Amazon Simple Storage
	// Service"); users know them by their brand names. Fall through to
	// the AWS-supplied display_name for anything not in this map.
	const BRAND_ALIAS: Record<string, string> = {
		s3: 'Amazon S3',
		lambda: 'AWS Lambda',
		dynamodb: 'Amazon DynamoDB',
		sqs: 'Amazon SQS',
		sns: 'Amazon SNS',
		kms: 'AWS KMS',
		secretsmanager: 'AWS Secrets Manager',
		events: 'Amazon EventBridge',
		apigateway: 'Amazon API Gateway',
		states: 'AWS Step Functions',
		ses: 'Amazon SES',
		monitoring: 'Amazon CloudWatch',
		route53: 'Amazon Route 53',
		kinesis: 'Amazon Kinesis Data Streams',
		cloudfront: 'Amazon CloudFront',
		firehose: 'Amazon Data Firehose',
		logs: 'Amazon CloudWatch Logs',
		ecr: 'Amazon ECR',
		'cognito-idp': 'Amazon Cognito User Pools',
		'cognito-identity': 'Amazon Cognito Identity',
		ec2: 'Amazon EC2',
		rds: 'Amazon RDS',
	};

	// Stable tints per service so the same colour represents the same
	// service across the chart + per-service cards.
	const SERVICE_TINTS: Record<string, string> = {
		s3: 'oklch(70% 0.15 25)', // warm orange
		lambda: 'oklch(70% 0.15 145)', // green
		dynamodb: 'oklch(70% 0.15 250)', // blue
		sqs: 'oklch(70% 0.15 320)', // pink/magenta
		sns: 'oklch(72% 0.15 60)', // amber
		kms: 'oklch(70% 0.15 200)', // teal
		secretsmanager: 'oklch(68% 0.15 285)', // violet
		events: 'oklch(70% 0.15 105)', // chartreuse
		apigateway: 'oklch(68% 0.15 0)', // red/coral
		states: 'oklch(70% 0.13 220)', // sky blue
		ses: 'oklch(72% 0.13 175)', // mint
		monitoring: 'oklch(68% 0.15 340)', // hot pink
		route53: 'oklch(72% 0.12 50)', // peach
		kinesis: 'oklch(68% 0.14 130)', // grass
		cloudfront: 'oklch(72% 0.13 95)', // gold
		firehose: 'oklch(68% 0.16 30)', // burnt orange
		logs: 'oklch(64% 0.12 240)', // dusk blue
		ecr: 'oklch(70% 0.13 280)', // lavender
		'cognito-idp': 'oklch(72% 0.14 165)', // jade
		'cognito-identity': 'oklch(70% 0.12 195)', // teal
		ec2: 'oklch(70% 0.16 35)', // tangerine
		rds: 'oklch(68% 0.13 250)', // navy
	};
	const FALLBACK_TINT = 'oklch(70% 0.05 0)';

	let report = $state<BillingReport | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let lastFetched = $state<number>(0);
	let timer: ReturnType<typeof setInterval> | undefined;
	let history = $state<HistoryPoint[]>([]);

	// Money you'd light on fire in a year before going broke. Default
	// $1k — a reasonable "side project gets DDoSed" budget. Persisted so
	// users can dial it to their actual fear threshold.
	const BUDGET_KEY = 'awsim-billing-budget';
	const BUDGET_PRESETS = [100, 1_000, 10_000, 100_000];
	let budgetUsd = $state(1_000);
	let editingBudget = $state(false);
	let budgetInputEl: HTMLInputElement | null = $state(null);

	async function load() {
		loading = true;
		try {
			report = await fetchBilling();
			error = null;
			lastFetched = Date.now();
			recordHistory(report);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load billing report';
			toast.error(error);
		} finally {
			loading = false;
		}
	}

	function recordHistory(r: BillingReport) {
		const now = Date.now();
		const next = [
			...history,
			{
				ts: now,
				running_cost_usd: r.running_cost_usd,
				projected_monthly_cost_usd: r.projected_monthly_cost_usd,
			},
		].filter((p) => now - p.ts <= HISTORY_WINDOW_MS);
		history = next;
		try {
			localStorage.setItem(HISTORY_KEY, JSON.stringify(next));
		} catch {
			/* ignore — quota exceeded etc. */
		}
	}

	function loadHistory() {
		try {
			const saved = localStorage.getItem(HISTORY_KEY);
			if (!saved) return;
			const parsed = JSON.parse(saved) as HistoryPoint[];
			if (!Array.isArray(parsed)) return;
			const now = Date.now();
			history = parsed.filter(
				(p) =>
					typeof p?.ts === 'number' &&
					typeof p?.running_cost_usd === 'number' &&
					now - p.ts <= HISTORY_WINDOW_MS,
			);
		} catch {
			/* ignore */
		}
	}

	onMount(() => {
		try {
			const saved = localStorage.getItem(BUDGET_KEY);
			if (saved) {
				const parsed = Number(saved);
				if (Number.isFinite(parsed) && parsed > 0) budgetUsd = parsed;
			}
		} catch {
			/* ignore */
		}
		loadHistory();
		void load();
		timer = setInterval(load, REFRESH_INTERVAL_MS);
	});

	onDestroy(() => {
		if (timer) clearInterval(timer);
	});

	function persistBudget() {
		try {
			localStorage.setItem(BUDGET_KEY, String(budgetUsd));
		} catch {
			/* ignore */
		}
	}

	function setBudget(value: number) {
		if (Number.isFinite(value) && value > 0) {
			budgetUsd = Math.round(value);
			persistBudget();
		}
	}

	async function startEditingBudget() {
		editingBudget = true;
		// Wait for the input to render, then focus + select for fast retype.
		await Promise.resolve();
		budgetInputEl?.focus();
		budgetInputEl?.select();
	}

	function commitBudgetEdit(rawValue: string) {
		const parsed = Number(rawValue.replace(/[,$\s]/g, ''));
		if (Number.isFinite(parsed) && parsed > 0) {
			setBudget(parsed);
		}
		editingBudget = false;
	}

	function fmtBudget(n: number): string {
		if (n >= 1_000_000) return `$${(n / 1_000_000).toFixed(n % 1_000_000 === 0 ? 0 : 1)}M`;
		if (n >= 1_000) return `$${(n / 1_000).toFixed(n % 1_000 === 0 ? 0 : 1)}K`;
		return `$${n.toLocaleString('en-US')}`;
	}

	/** Dollar formatting with sub-cent and big-number friendliness. */
	function fmtUsd(n: number, opts?: { precise?: boolean }): string {
		if (n === 0) return '$0.00';
		const abs = Math.abs(n);
		// Below 1¢ — show "<$0.01" unless the caller wants the actual
		// fractional value (used in the per-row breakdown).
		if (abs < 0.01) {
			if (!opts?.precise) return n < 0 ? '> -$0.01' : '<$0.01';
			// Pick enough digits to show 2 significant figures.
			const exp = Math.floor(Math.log10(abs));
			const digits = Math.max(2, 1 - exp);
			return `$${n.toFixed(digits)}`;
		}
		if (abs >= 1_000_000) return `$${(n / 1_000_000).toFixed(2)}M`;
		if (abs >= 10_000) return `$${(n / 1_000).toFixed(1)}K`;
		return n.toLocaleString('en-US', {
			style: 'currency',
			currency: 'USD',
			minimumFractionDigits: 2,
			maximumFractionDigits: opts?.precise ? 4 : 2,
		});
	}

	function fmtBytes(bytes: number): string {
		if (bytes < 1024) return `${bytes} B`;
		const units = ['KiB', 'MiB', 'GiB', 'TiB'];
		let value = bytes / 1024;
		let i = 0;
		while (value >= 1024 && i < units.length - 1) {
			value /= 1024;
			i++;
		}
		return `${value.toFixed(value < 10 ? 2 : 1)} ${units[i]}`;
	}

	/**
	 * Per-request rates are tiny (1e-7 .. 1e-5 USD). Show them as
	 * "$X per Y requests" with Y picked so the cost is a friendly
	 * number — that's how AWS marketing copy reads.
	 */
	function fmtRate(usdPerRequest: number): string {
		if (usdPerRequest === 0) return 'free';
		// Step up the divisor until we land on at least $0.001.
		const tiers: { divisor: number; label: string }[] = [
			{ divisor: 1, label: 'req' },
			{ divisor: 1_000, label: '1K' },
			{ divisor: 1_000_000, label: '1M' },
			{ divisor: 1_000_000_000, label: '1B' },
		];
		for (const t of tiers) {
			const cost = usdPerRequest * t.divisor;
			if (cost >= 0.001) {
				return `$${cost.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 4 })} / ${t.label}`;
			}
		}
		// Truly tiny — fall back to scientific.
		return `$${usdPerRequest.toExponential(2)} / req`;
	}

	function fmtNumber(n: number): string {
		if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
		if (n >= 10_000) return `${(n / 1_000).toFixed(1)}K`;
		return n.toLocaleString('en-US');
	}

	function fmtElapsed(secs: number): string {
		if (secs < 60) return `${secs}s`;
		if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
		const h = Math.floor(secs / 3600);
		const m = Math.floor((secs % 3600) / 60);
		return `${h}h ${m}m`;
	}

	/**
	 * AWS pricing descriptions are like "$0.005 per 1,000 PUT/COPY/POST
	 * or LIST requests" — the price prefix duplicates the rate column
	 * we already render. Strip it and the trailing region note for a
	 * clean per-row label.
	 */
	function cleanDescription(desc: string): string {
		if (!desc) return '';
		let s = desc;
		// Strip "$X per Y unit" prefix (S3 / DDB style descriptions).
		s = s.replace(/^\$[\d.]+ per [\d,]+\s*(million|billion|GB|TB|MB|hours?|requests?)?\s*/i, '');
		s = s.replace(/^\$[\d.]+\s*per\s+[A-Za-z]+\s*/, '');
		// Strip trailing region notes — both "(N. Virginia)" form, the
		// dash-separated " - US East (...)" form Lambda uses, and the
		// space-prefixed " in US East" form KMS / EventBridge use.
		s = s.replace(/\s*-\s*US\s+East.*$/i, '');
		s = s.replace(/\s*in\s+US\s+East.*$/i, '');
		s = s.replace(/\s*\([^)]+\)\s*$/, '');
		// Lambda's headline description still leads with the service
		// name ("AWS Lambda - Total Requests"); strip it so the column
		// reads cleanly.
		s = s.replace(/^AWS\s+Lambda\s*-\s*/i, '');
		s = s.replace(/^Amazon\s+\S+\s*-\s*/i, '');
		return s.trim() || desc;
	}

	function brandName(svc: BillingService): string {
		return BRAND_ALIAS[svc.service] ?? svc.display_name;
	}

	function tintFor(service: string): string {
		return SERVICE_TINTS[service] ?? FALLBACK_TINT;
	}

	let totalCost = $derived(report?.running_cost_usd ?? 0);

	let serviceShares = $derived.by(() => {
		if (!report || totalCost <= 0) return [];
		return report.services
			.filter((s) => s.total_cost_usd > 0)
			.map((s) => ({
				service: s.service,
				name: brandName(s),
				cost: s.total_cost_usd,
				pct: (s.total_cost_usd / totalCost) * 100,
				tint: tintFor(s.service),
			}));
	});

	let daysToBudget = $derived.by(() => {
		if (!report || report.projected_monthly_cost_usd <= 0) return null;
		const dailyBurn = report.projected_monthly_cost_usd / 30;
		return budgetUsd / dailyBurn;
	});

	let bankruptcyText = $derived.by(() => {
		if (daysToBudget == null) return '∞';
		if (daysToBudget >= 365 * 100) return '∞';
		if (daysToBudget >= 365) return `${(daysToBudget / 365).toFixed(1)}y`;
		if (daysToBudget >= 30) return `${(daysToBudget / 30).toFixed(1)}mo`;
		if (daysToBudget >= 1) return `${daysToBudget.toFixed(1)}d`;
		const hours = daysToBudget * 24;
		if (hours >= 1) return `${hours.toFixed(1)}h`;
		return `${(hours * 60).toFixed(1)}m`;
	});

	let bankruptcySublabel = $derived.by(() => {
		if (daysToBudget == null) return 'no burn yet';
		if (daysToBudget >= 365 * 100) return 'no burn yet';
		return 'until budget exhausted';
	});

	// SVG viewBox for the sparkline chart (logical coords).
	const CHART_W = 600;
	const CHART_H = 80;

	// Hover state for the chart tooltip — null when the cursor is
	// outside the chart, otherwise the index into history that's
	// closest to the cursor's x position.
	let hoverIndex = $state<number | null>(null);
	let chartContainerEl: HTMLDivElement | null = $state(null);

	function onChartMove(e: MouseEvent) {
		const target = e.currentTarget as HTMLElement;
		if (!target || history.length < 2) return;
		const rect = target.getBoundingClientRect();
		const fraction = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
		const minTs = history[0].ts;
		const maxTs = Math.max(history[history.length - 1].ts, minTs + 1);
		const targetTs = minTs + fraction * (maxTs - minTs);
		// Pick the closest sample by timestamp.
		let best = 0;
		let bestDelta = Infinity;
		for (let i = 0; i < history.length; i++) {
			const delta = Math.abs(history[i].ts - targetTs);
			if (delta < bestDelta) {
				bestDelta = delta;
				best = i;
			}
		}
		hoverIndex = best;
	}

	function onChartLeave() {
		hoverIndex = null;
	}

	let hoverPoint = $derived.by(() => {
		if (hoverIndex == null || !chartPaths || hoverIndex >= history.length) return null;
		const p = history[hoverIndex];
		const minTs = history[0].ts;
		const maxTs = Math.max(history[history.length - 1].ts, minTs + 1);
		const span = maxTs - minTs;
		const xPct = ((p.ts - minTs) / span) * 100;
		const yPct = (1 - p.running_cost_usd / Math.max(chartPaths.maxCost, 1e-12)) * (100 - 5);
		return { ...p, xPct, yPct };
	});

	let chartPaths = $derived.by(() => {
		if (history.length < 2) return null;
		const minTs = history[0].ts;
		const maxTs = Math.max(history[history.length - 1].ts, minTs + 1);
		const span = maxTs - minTs;
		const maxCost = Math.max(...history.map((p) => p.running_cost_usd), 1e-12);
		const xOf = (ts: number) => ((ts - minTs) / span) * CHART_W;
		const yOf = (cost: number) => CHART_H - (cost / maxCost) * (CHART_H - 4);
		const pts = history
			.map((p) => `${xOf(p.ts).toFixed(2)},${yOf(p.running_cost_usd).toFixed(2)}`)
			.join(' L ');
		const stroke = `M ${pts}`;
		const area = `M ${xOf(minTs).toFixed(2)},${CHART_H} L ${pts} L ${xOf(maxTs).toFixed(2)},${CHART_H} Z`;
		const last = history[history.length - 1];
		const lastPoint = { x: xOf(last.ts), y: yOf(last.running_cost_usd) };
		return { stroke, area, maxCost, minTs, maxTs, lastPoint };
	});

	function fmtRelative(ts: number): string {
		const diff = Math.max(0, Date.now() - ts);
		const mins = Math.floor(diff / 60000);
		const secs = Math.floor((diff % 60000) / 1000);
		if (mins === 0) return `${secs}s ago`;
		if (mins < 60) return `${mins}m ago`;
		const h = Math.floor(mins / 60);
		return `${h}h ago`;
	}

	type BurnSeverity = 'safe' | 'warn' | 'critical';
	let burnSeverity = $derived.by<BurnSeverity>(() => {
		if (daysToBudget == null) return 'safe';
		if (daysToBudget < 7) return 'critical';
		if (daysToBudget < 90) return 'warn';
		return 'safe';
	});

	const BURN_CLASSES: Record<BurnSeverity, string> = {
		safe: 'border-border bg-card',
		warn: 'border-amber-500 bg-amber-500/10',
		critical: 'border-destructive bg-destructive/10',
	};
</script>

<svelte:head>
	<title>AWSim · Billing</title>
</svelte:head>

<ServicePage
	title="Billing (estimated)"
	description="Rolling AWS bill from metered usage × vendored pricing."
>
	{#snippet actions()}
		<Button variant="ghost" size="sm" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	{/snippet}

	<div class="flex flex-col gap-4 p-6">
		<!-- Headline grid: projected monthly is the hero, the other two
		     hang off to its right -->
		<div class="grid gap-3 lg:grid-cols-[2fr_1fr_1fr]">
			<div
				class="relative overflow-hidden rounded-lg border border-border bg-gradient-to-br from-card to-card/50 p-5"
			>
				<div class="flex items-center gap-2 text-xs uppercase tracking-wider text-muted-foreground">
					<TrendingUpIcon class="size-3.5" />
					Projected monthly bill
				</div>
				<div class="mt-3 font-mono text-5xl font-bold tabular-nums tracking-tight">
					{report ? fmtUsd(report.projected_monthly_cost_usd) : '—'}
				</div>
				<div class="mt-2 text-xs text-muted-foreground">
					Linear extrapolation from {report ? fmtElapsed(report.elapsed_secs) : '—'} of metered traffic.
				</div>

				<!-- Stacked share bar -->
				{#if serviceShares.length > 0}
					<div class="mt-4 space-y-2">
						<div class="flex h-3 w-full overflow-hidden rounded-full bg-muted/40">
							{#each serviceShares as s (s.service)}
								<div
									class="h-full transition-all duration-500"
									style="width: {s.pct}%; background-color: {s.tint};"
									title="{s.name} — {fmtUsd(s.cost, { precise: true })} ({s.pct.toFixed(1)}%)"
								></div>
							{/each}
						</div>
						<div class="flex flex-wrap gap-x-3 gap-y-1 text-[11px]">
							{#each serviceShares as s (s.service)}
								<div class="flex items-center gap-1.5">
									<span class="size-2 rounded-full" style="background-color: {s.tint};"></span>
									<span class="text-muted-foreground">{s.name}</span>
									<span class="font-mono tabular-nums">{s.pct.toFixed(1)}%</span>
								</div>
							{/each}
						</div>
					</div>
				{/if}
			</div>

			<div class="rounded-lg border border-border bg-card p-4">
				<div class="flex items-center gap-2 text-xs uppercase tracking-wider text-muted-foreground">
					<DollarSignIcon class="size-3.5" />
					Spent so far
				</div>
				<div class="mt-3 font-mono text-2xl font-semibold tabular-nums">
					{report ? fmtUsd(report.running_cost_usd, { precise: true }) : '—'}
				</div>
				{#if report}
					<div class="mt-2 flex items-center gap-1 text-xs text-muted-foreground">
						<ClockIcon class="size-3" />
						over {fmtElapsed(report.elapsed_secs)}
					</div>
				{/if}
			</div>

			<div
				class="rounded-lg border p-4 transition-colors duration-300 {BURN_CLASSES[burnSeverity]}"
			>
				<div class="flex items-center gap-2 text-xs uppercase tracking-wider text-muted-foreground">
					<SkullIcon class="size-3.5" />
					Time to bankruptcy
				</div>
				<div class="mt-3 font-mono text-2xl font-semibold tabular-nums">{bankruptcyText}</div>
				<div class="mt-2 text-xs text-muted-foreground">
					until
					{#if editingBudget}
						<input
							bind:this={budgetInputEl}
							type="text"
							inputmode="numeric"
							value={budgetUsd}
							onblur={(e) => commitBudgetEdit(e.currentTarget.value)}
							onkeydown={(e) => {
								if (e.key === 'Enter') {
									e.preventDefault();
									commitBudgetEdit(e.currentTarget.value);
								} else if (e.key === 'Escape') {
									editingBudget = false;
								}
							}}
							class="inline w-20 rounded border border-border bg-background px-1 py-0 text-xs font-mono tabular-nums focus:outline-none focus:ring-1 focus:ring-ring"
						/>
					{:else}
						<button
							type="button"
							onclick={startEditingBudget}
							class="group inline-flex items-center gap-1 rounded border border-transparent px-1 font-mono font-medium text-foreground tabular-nums underline decoration-dotted decoration-muted-foreground/60 underline-offset-2 transition-colors hover:bg-muted/40 hover:decoration-foreground/80 focus:outline-none focus:ring-1 focus:ring-ring"
							title="Click to edit budget"
						>
							{fmtBudget(budgetUsd)}
							<PencilIcon class="size-2.5 opacity-50 transition-opacity group-hover:opacity-100" />
						</button>
					{/if}
					budget exhausted
				</div>
				<div class="mt-2 flex flex-wrap gap-1">
					{#each BUDGET_PRESETS as preset (preset)}
						<button
							type="button"
							onclick={() => setBudget(preset)}
							class="rounded border px-1.5 py-0.5 text-[10px] font-mono tabular-nums transition-colors {budgetUsd === preset
								? 'border-foreground/40 bg-foreground/10 text-foreground'
								: 'border-border text-muted-foreground hover:bg-muted/40 hover:text-foreground'}"
						>
							{fmtBudget(preset)}
						</button>
					{/each}
					<button
						type="button"
						onclick={startEditingBudget}
						class="rounded border px-1.5 py-0.5 text-[10px] transition-colors {!BUDGET_PRESETS.includes(budgetUsd)
							? 'border-foreground/40 bg-foreground/10 text-foreground'
							: 'border-border text-muted-foreground hover:bg-muted/40 hover:text-foreground'}"
					>
						Custom…
					</button>
				</div>
			</div>
		</div>

		<!-- Cost trajectory: rolling 30-min running-cost sparkline,
		     fed from localStorage so the chart survives page reloads.
		     Hovering shows an exact $-value tooltip at the cursor. -->
		{#if chartPaths}
			<div class="rounded-lg border border-border bg-card p-4">
				<div class="flex items-baseline justify-between">
					<div class="flex items-center gap-2 text-xs uppercase tracking-wider text-muted-foreground">
						<TrendingUpIcon class="size-3.5" />
						Cost trajectory
					</div>
					<div class="text-[10px] text-muted-foreground">
						last {Math.round(HISTORY_WINDOW_MS / 60000)} min
						· peak {fmtUsd(chartPaths.maxCost, { precise: true })}
					</div>
				</div>
				<div
					bind:this={chartContainerEl}
					role="img"
					aria-label="Cost trajectory chart"
					class="relative mt-3"
					onmousemove={onChartMove}
					onmouseleave={onChartLeave}
				>
				<svg
					viewBox="0 0 {CHART_W} {CHART_H}"
					preserveAspectRatio="none"
					class="h-28 w-full"
					aria-label="Running cost over time"
				>
					<defs>
						<linearGradient id="cost-fill" x1="0" y1="0" x2="0" y2="1">
							<stop offset="0%" stop-color="oklch(70% 0.15 25)" stop-opacity="0.45" />
							<stop offset="100%" stop-color="oklch(70% 0.15 25)" stop-opacity="0" />
						</linearGradient>
					</defs>
					<!-- Subtle baseline so the curve has visual context. -->
					<line
						x1="0"
						x2={CHART_W}
						y1={CHART_H}
						y2={CHART_H}
						stroke="oklch(50% 0.02 0)"
						stroke-width="1"
						vector-effect="non-scaling-stroke"
					/>
					<path d={chartPaths.area} fill="url(#cost-fill)" />
					<path
						d={chartPaths.stroke}
						fill="none"
						stroke="oklch(70% 0.15 25)"
						stroke-width="2"
						stroke-linejoin="round"
						stroke-linecap="round"
						vector-effect="non-scaling-stroke"
					/>
					<!-- Most-recent-value dot anchors the eye on the curve tip. -->
					{#if chartPaths.lastPoint}
						<circle
							cx={chartPaths.lastPoint.x}
							cy={chartPaths.lastPoint.y}
							r="2"
							fill="oklch(70% 0.15 25)"
							vector-effect="non-scaling-stroke"
						/>
					{/if}
				</svg>
				<!-- Hover overlay: vertical guide + tooltip pinned to
				     the closest sample's x position. -->
				{#if hoverPoint}
					<div
						class="pointer-events-none absolute top-0 bottom-0 w-px bg-foreground/30"
						style="left: {hoverPoint.xPct}%;"
					></div>
					<div
						class="pointer-events-none absolute z-10 rounded-md border border-border bg-popover px-2 py-1 text-[10px] shadow-md"
						style="left: {Math.min(hoverPoint.xPct, 80)}%; top: {Math.max(hoverPoint.yPct - 12, 0)}%; transform: translateX({hoverPoint.xPct > 80 ? '-100%' : '6px'});"
					>
						<div class="font-mono font-semibold tabular-nums">{fmtUsd(hoverPoint.running_cost_usd, { precise: true })}</div>
						<div class="text-muted-foreground">{fmtRelative(hoverPoint.ts)}</div>
					</div>
				{/if}
				</div>
				<div class="flex justify-between text-[10px] text-muted-foreground">
					<span>{fmtRelative(chartPaths.minTs)}</span>
					<span>now</span>
				</div>
			</div>
		{/if}

		<!-- Per-service breakdown -->
		{#if report && report.services.length > 0}
			<div class="flex flex-col gap-3">
				{#each report.services as svc (svc.service)}
					{@const tint = tintFor(svc.service)}
					{@const svcMaxDimCost = Math.max(
						...svc.dimensions.map((d) => d.cost_usd),
						svc.data_transfer_out_cost_usd,
						svc.data_ingest_cost_usd,
						svc.storage_cost_usd,
						svc.compute_cost_usd,
						svc.resource_cost_usd,
						0,
					)}
					<div class="overflow-hidden rounded-lg border border-border bg-card">
						<!-- 4px coloured strip on the left ties the row to the chart -->
						<div class="flex items-stretch">
							<div class="w-1 shrink-0" style="background-color: {tint};"></div>
							<div class="flex-1">
								<div class="flex items-start justify-between gap-3 border-b border-border px-4 py-3">
									<div class="min-w-0">
										<div class="flex items-center gap-2">
											<h3 class="text-sm font-semibold">{brandName(svc)}</h3>
											<Badge variant="outline" class="h-4 px-1.5 text-[10px] font-mono">
												{svc.region}
											</Badge>
										</div>
										<div class="mt-0.5 truncate text-xs text-muted-foreground">
											{fmtNumber(svc.request_count)} requests
											• {fmtBytes(svc.bytes_in)} in / {fmtBytes(svc.bytes_out)} out
											{#if svc.error_count > 0}
												• <span class="text-destructive">{fmtNumber(svc.error_count)} errors</span>
											{/if}
										</div>
									</div>
									<div class="text-right">
										<div class="font-mono text-lg font-semibold tabular-nums">
											{fmtUsd(svc.total_cost_usd, { precise: true })}
										</div>
										{#if totalCost > 0}
											<div class="text-[10px] text-muted-foreground">
												{((svc.total_cost_usd / totalCost) * 100).toFixed(0)}% of bill
											</div>
										{/if}
									</div>
								</div>
								<table class="w-full text-xs">
									<thead>
										<tr class="text-left text-muted-foreground">
											<th class="px-4 py-1.5 font-normal">Dimension</th>
											<th class="px-2 py-1.5 text-right font-normal">Rate</th>
											<th class="px-2 py-1.5 text-right font-normal">Count</th>
											<th class="px-2 py-1.5 text-right font-normal">Cost</th>
											<th class="px-4 py-1.5 font-normal" style="width: 80px;"></th>
										</tr>
									</thead>
									<tbody>
										{#each svc.dimensions as dim, i (svc.service + ':' + i + ':' + dim.description)}
											{@const sharePct =
												svcMaxDimCost > 0 ? (dim.cost_usd / svcMaxDimCost) * 100 : 0}
											<tr class="border-t border-border/40">
												<td class="px-4 py-1.5">{cleanDescription(dim.description)}</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums text-muted-foreground">
													{fmtRate(dim.price_per_request)}
												</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums">
													{fmtNumber(dim.request_count)}
												</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums">
													{fmtUsd(dim.cost_usd, { precise: true })}
												</td>
												<td class="px-4 py-1.5">
													<div class="h-1 w-full overflow-hidden rounded-full bg-muted/30">
														<div
															class="h-full transition-all duration-500"
															style="width: {sharePct}%; background-color: {tint}; opacity: 0.7;"
														></div>
													</div>
												</td>
											</tr>
										{/each}
										{#if svc.data_transfer_out_cost_usd > 0}
											{@const sharePct =
												svcMaxDimCost > 0
													? (svc.data_transfer_out_cost_usd / svcMaxDimCost) * 100
													: 0}
											<tr class="border-t border-border/40">
												<td class="px-4 py-1.5">Data transfer out (internet egress)</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums text-muted-foreground">
													${(svc.data_transfer_out_cost_usd / (svc.bytes_out / 1_073_741_824)).toFixed(2)}/GB
												</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums text-muted-foreground">
													{fmtBytes(svc.bytes_out)}
												</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums">
													{fmtUsd(svc.data_transfer_out_cost_usd, { precise: true })}
												</td>
												<td class="px-4 py-1.5">
													<div class="h-1 w-full overflow-hidden rounded-full bg-muted/30">
														<div
															class="h-full transition-all duration-500"
															style="width: {sharePct}%; background-color: {tint}; opacity: 0.7;"
														></div>
													</div>
												</td>
											</tr>
										{/if}
										{#if svc.data_ingest_cost_usd > 0}
											{@const sharePct =
												svcMaxDimCost > 0
													? (svc.data_ingest_cost_usd / svcMaxDimCost) * 100
													: 0}
											<tr class="border-t border-border/40">
												<td class="px-4 py-1.5">Data ingested</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums text-muted-foreground">
													${(svc.data_ingest_cost_usd / (svc.bytes_in / 1_073_741_824)).toFixed(3)}/GB
												</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums text-muted-foreground">
													{fmtBytes(svc.bytes_in)}
												</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums">
													{fmtUsd(svc.data_ingest_cost_usd, { precise: true })}
												</td>
												<td class="px-4 py-1.5">
													<div class="h-1 w-full overflow-hidden rounded-full bg-muted/30">
														<div
															class="h-full transition-all duration-500"
															style="width: {sharePct}%; background-color: {tint}; opacity: 0.7;"
														></div>
													</div>
												</td>
											</tr>
										{/if}
										{#if svc.storage_bytes > 0 || svc.storage_cost_usd > 0}
											{@const sharePct =
												svcMaxDimCost > 0
													? (svc.storage_cost_usd / svcMaxDimCost) * 100
													: 0}
											<tr class="border-t border-border/40">
												<td class="px-4 py-1.5">Storage (at-rest GB-month)</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums text-muted-foreground">
													point-in-time
												</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums text-muted-foreground">
													{fmtBytes(svc.storage_bytes)}
												</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums">
													{fmtUsd(svc.storage_cost_usd, { precise: true })}
												</td>
												<td class="px-4 py-1.5">
													<div class="h-1 w-full overflow-hidden rounded-full bg-muted/30">
														<div
															class="h-full transition-all duration-500"
															style="width: {sharePct}%; background-color: {tint}; opacity: 0.7;"
														></div>
													</div>
												</td>
											</tr>
										{/if}
										{#if svc.resource_count > 0 || svc.resource_cost_usd > 0}
											{@const sharePct =
												svcMaxDimCost > 0
													? (svc.resource_cost_usd / svcMaxDimCost) * 100
													: 0}
											<tr class="border-t border-border/40">
												<td class="px-4 py-1.5">Running instances (instance-hours)</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums text-muted-foreground">
													baseline
												</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums text-muted-foreground">
													{svc.resource_count}
												</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums">
													{fmtUsd(svc.resource_cost_usd, { precise: true })}
												</td>
												<td class="px-4 py-1.5">
													<div class="h-1 w-full overflow-hidden rounded-full bg-muted/30">
														<div
															class="h-full transition-all duration-500"
															style="width: {sharePct}%; background-color: {tint}; opacity: 0.7;"
														></div>
													</div>
												</td>
											</tr>
										{/if}
										{#if svc.compute_gb_seconds > 0 || svc.compute_cost_usd > 0}
											{@const sharePct =
												svcMaxDimCost > 0
													? (svc.compute_cost_usd / svcMaxDimCost) * 100
													: 0}
											<tr class="border-t border-border/40">
												<td class="px-4 py-1.5">Compute (GB-seconds)</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums text-muted-foreground">
													$0.0000167 / GB-s
												</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums text-muted-foreground">
													{svc.compute_gb_seconds.toFixed(svc.compute_gb_seconds < 1 ? 4 : 2)}&nbsp;GB·s
												</td>
												<td class="px-2 py-1.5 text-right font-mono tabular-nums">
													{fmtUsd(svc.compute_cost_usd, { precise: true })}
												</td>
												<td class="px-4 py-1.5">
													<div class="h-1 w-full overflow-hidden rounded-full bg-muted/30">
														<div
															class="h-full transition-all duration-500"
															style="width: {sharePct}%; background-color: {tint}; opacity: 0.7;"
														></div>
													</div>
												</td>
											</tr>
										{/if}
									</tbody>
								</table>
							</div>
						</div>
					</div>
				{/each}
			</div>
		{:else if !loading && !error}
			<EmptyState
				icon={DollarSignIcon}
				title="No metered usage yet"
				description="Hit any S3, Lambda or DynamoDB endpoint and the meter will start the bill rolling."
			/>
		{/if}

		{#if lastFetched > 0}
			<div class="text-right text-[10px] text-muted-foreground">
				auto-refreshes every {Math.round(REFRESH_INTERVAL_MS / 1000)}s
			</div>
		{/if}
	</div>
</ServicePage>
