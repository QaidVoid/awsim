<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { fetchDebugObjects, type DebugObjectsPayload } from '$lib/api';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Camera from '@lucide/svelte/icons/camera';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Pause from '@lucide/svelte/icons/pause';
	import Play from '@lucide/svelte/icons/play';

	const POLL_MS = 5000;
	const HISTORY_MAX = 60; // 5 minutes at 5s tick

	let current = $state<DebugObjectsPayload | null>(null);
	let baseline = $state<DebugObjectsPayload | null>(null);
	let history = $state<{ t: number; rss: number }[]>([]);
	let polling = $state(true);
	let timer: ReturnType<typeof setInterval> | null = null;

	onMount(() => {
		void load();
		startPolling();
	});

	onDestroy(() => stopPolling());

	function startPolling() {
		stopPolling();
		timer = setInterval(() => void load(), POLL_MS);
	}
	function stopPolling() {
		if (timer) clearInterval(timer);
		timer = null;
	}

	async function load() {
		try {
			const d = await fetchDebugObjects();
			current = d;
			if (d.process.rss_bytes != null) {
				history = [...history, { t: d.captured_at, rss: d.process.rss_bytes }].slice(
					-HISTORY_MAX
				);
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load diagnostic');
		}
	}

	function togglePolling() {
		polling = !polling;
		if (polling) startPolling();
		else stopPolling();
	}

	function snapshotBaseline() {
		if (!current) return;
		baseline = current;
		toast.success('Baseline captured — keep using the app, then refresh deltas');
	}

	function clearBaseline() {
		baseline = null;
	}

	function fmtBytes(n: number | null | undefined): string {
		if (n == null) return '—';
		if (n < 1024) return `${n} B`;
		if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
		if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MiB`;
		return `${(n / 1024 / 1024 / 1024).toFixed(2)} GiB`;
	}

	function fmtNum(n: number | null | undefined): string {
		if (n == null) return '—';
		return n.toLocaleString();
	}

	function delta(now: number | null | undefined, base: number | null | undefined): string {
		if (now == null || base == null) return '';
		const d = now - base;
		if (d === 0) return '±0';
		const sign = d > 0 ? '+' : '';
		return `${sign}${d.toLocaleString()}`;
	}

	function deltaBytes(now: number | null | undefined, base: number | null | undefined): string {
		if (now == null || base == null) return '';
		const d = now - base;
		if (d === 0) return '±0';
		const sign = d > 0 ? '+' : '';
		return `${sign}${fmtBytes(Math.abs(d))}`;
	}

	function deltaClass(now: number | null | undefined, base: number | null | undefined): string {
		if (now == null || base == null) return 'text-muted-foreground';
		const d = now - base;
		if (d === 0) return 'text-muted-foreground';
		return d > 0 ? 'text-orange-400' : 'text-emerald-400';
	}

	const processStats = $derived.by(() => {
		if (!current) return [];
		return [
			{ label: 'RSS (now)', val: current.process.rss_bytes, base: baseline?.process.rss_bytes },
			{
				label: 'VmHWM (peak RSS)',
				val: current.process.vm_hwm_bytes,
				base: baseline?.process.vm_hwm_bytes
			},
			{
				label: 'VmSize',
				val: current.process.vm_size_bytes,
				base: baseline?.process.vm_size_bytes
			},
			{
				label: 'VmData',
				val: current.process.vm_data_bytes,
				base: baseline?.process.vm_data_bytes
			},
			{ label: 'VmPeak', val: current.process.vm_peak_bytes, base: baseline?.process.vm_peak_bytes }
		];
	});

	const appRows = $derived.by(() => {
		if (!current) return [];
		return [
			{
				label: 'Request details (ring)',
				val: current.app.request_details,
				base: baseline?.app.request_details
			},
			{
				label: 'SSE subscribers (req)',
				val: current.app.request_event_subscribers,
				base: baseline?.app.request_event_subscribers
			},
			{
				label: 'Subscribers (internal)',
				val: current.app.internal_event_subscribers,
				base: baseline?.app.internal_event_subscribers
			},
			{ label: 'Chaos rules', val: current.app.chaos_rules, base: baseline?.app.chaos_rules },
			{
				label: 'Chaos recent injections',
				val: current.app.chaos_recent_injections,
				base: baseline?.app.chaos_recent_injections
			},
			{
				label: 'Registered services',
				val: current.app.registered_services,
				base: baseline?.app.registered_services
			}
		];
	});

	const cogRows = $derived.by(() => {
		if (!current) return [];
		return [
			{
				label: 'User pools',
				val: current.cognito.user_pools,
				base: baseline?.cognito.user_pools
			},
			{
				label: 'Users (total)',
				val: current.cognito.totals.users,
				base: baseline?.cognito.totals.users
			},
			{
				label: 'Groups',
				val: current.cognito.totals.groups,
				base: baseline?.cognito.totals.groups
			},
			{
				label: 'Clients',
				val: current.cognito.totals.clients,
				base: baseline?.cognito.totals.clients
			},
			{
				label: 'Auth events',
				val: current.cognito.totals.auth_events,
				base: baseline?.cognito.totals.auth_events
			},
			{
				label: 'Devices',
				val: current.cognito.totals.devices,
				base: baseline?.cognito.totals.devices
			},
			{
				label: 'Revoked refresh tokens',
				val: current.cognito.totals.revoked_refresh_tokens,
				base: baseline?.cognito.totals.revoked_refresh_tokens
			},
			{
				label: 'MFA sessions',
				val: current.cognito.mfa_sessions,
				base: baseline?.cognito.mfa_sessions
			}
		];
	});

	const billRows = $derived.by(() => {
		if (!current) return [];
		return [
			{
				label: 'Account-region buckets',
				val: current.billing.account_region_buckets,
				base: baseline?.billing.account_region_buckets
			},
			{
				label: 'Op counters',
				val: current.billing.op_counters_total,
				base: baseline?.billing.op_counters_total
			},
			{
				label: 'Storage rows',
				val: current.billing.storage_rows_total,
				base: baseline?.billing.storage_rows_total
			},
			{
				label: 'Compute rows',
				val: current.billing.compute_rows_total,
				base: baseline?.billing.compute_rows_total
			},
			{
				label: 'Resource rows',
				val: current.billing.resource_rows_total,
				base: baseline?.billing.resource_rows_total
			}
		];
	});

	const sqlRows = $derived.by(() => {
		if (!current) return [];
		return [
			{
				label: 'CloudWatch Logs rows',
				val: current.sqlite.cloudwatch_logs_rows,
				base: baseline?.sqlite.cloudwatch_logs_rows
			},
			{
				label: 'CloudWatch Metrics rows',
				val: current.sqlite.cloudwatch_metrics_rows,
				base: baseline?.sqlite.cloudwatch_metrics_rows
			},
			{
				label: 'Kinesis records',
				val: current.sqlite.kinesis_rows,
				base: baseline?.sqlite.kinesis_rows
			},
			{
				label: 'SES sent emails',
				val: current.sqlite.ses_rows,
				base: baseline?.sqlite.ses_rows
			}
		];
	});

	// Sparkline path — the "polyline approach": 60 points across width, height clamped.
	function sparkPath(width: number, height: number): string {
		if (history.length < 2) return '';
		const min = Math.min(...history.map((h) => h.rss));
		const max = Math.max(...history.map((h) => h.rss));
		const range = Math.max(1, max - min);
		const dx = width / Math.max(1, history.length - 1);
		return history
			.map((h, i) => {
				const x = i * dx;
				const y = height - ((h.rss - min) / range) * height;
				return `${i === 0 ? 'M' : 'L'} ${x.toFixed(1)} ${y.toFixed(1)}`;
			})
			.join(' ');
	}
</script>

<ServicePage
	title="Observability"
	description="Live process memory + per-subsystem object counts. Snapshot a baseline, run a workload, watch what grew."
>
	{#snippet actions()}
		<Button size="sm" variant="ghost" onclick={togglePolling}>
			{#if polling}
				<Pause class="size-3.5" /> Pause
			{:else}
				<Play class="size-3.5" /> Resume
			{/if}
		</Button>
		<Button size="sm" variant="ghost" onclick={() => void load()} title="Refresh now">
			<RefreshCw class="size-3.5" />
		</Button>
		<Button size="sm" onclick={snapshotBaseline} disabled={!current}>
			<Camera class="size-3.5" /> Snapshot baseline
		</Button>
		{#if baseline}
			<Button size="sm" variant="ghost" onclick={clearBaseline}>
				<Trash2 class="size-3.5" /> Clear
			</Button>
		{/if}
	{/snippet}

	<div class="flex h-full min-h-0 flex-col gap-4 overflow-y-auto p-6">
		{#if !current}
			<p class="text-sm text-muted-foreground">Loading diagnostic...</p>
		{:else}
			<!-- Process — RSS sparkline + headline numbers -->
			<section class="rounded-lg border border-border bg-card p-4">
				<div class="mb-3 flex items-baseline justify-between">
					<h2 class="text-sm font-semibold uppercase tracking-wide text-muted-foreground">
						Process memory
					</h2>
					<span class="text-xs text-muted-foreground">
						uptime {Math.floor(current.app.uptime_secs / 60)}m {current.app.uptime_secs % 60}s
						· {current.app.request_count.toLocaleString()} requests handled
					</span>
				</div>

				<div class="mb-3 grid grid-cols-2 gap-3 sm:grid-cols-5">
					{#each processStats as s (s.label)}
						<div class="rounded border border-border/60 bg-background p-3">
							<div class="text-xs text-muted-foreground">{s.label}</div>
							<div class="font-mono text-base">{fmtBytes(s.val)}</div>
							{#if baseline}
								<div class="font-mono text-xs {deltaClass(s.val, s.base)}">
									{deltaBytes(s.val, s.base)}
								</div>
							{/if}
						</div>
					{/each}
				</div>

				<div class="rounded border border-border/60 bg-background p-3">
					<div class="mb-1 flex items-baseline justify-between">
						<span class="text-xs text-muted-foreground">
							RSS history — last {history.length} samples ({Math.round(
								(history.length * POLL_MS) / 1000
							)}s)
						</span>
						{#if history.length > 1}
							<span
								class="font-mono text-xs {deltaClass(
									history[history.length - 1].rss,
									history[0].rss
								)}"
							>
								{deltaBytes(history[history.length - 1].rss, history[0].rss)} since first sample
							</span>
						{/if}
					</div>
					<svg viewBox="0 0 600 80" class="h-20 w-full" preserveAspectRatio="none">
						{#if history.length >= 2}
							<path
								d={sparkPath(600, 80)}
								fill="none"
								stroke="rgb(251 146 60)"
								stroke-width="1.5"
								vector-effect="non-scaling-stroke"
							/>
						{:else}
							<text x="300" y="40" text-anchor="middle" class="fill-muted-foreground text-xs">
								Collecting samples...
							</text>
						{/if}
					</svg>
				</div>
			</section>

			<!-- App-level structures -->
			<section class="rounded-lg border border-border bg-card p-4">
				<h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
					Gateway / app
				</h2>
				<div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
					{#each appRows as r (r.label)}
						<div class="rounded border border-border/60 bg-background p-3">
							<div class="text-xs text-muted-foreground">{r.label}</div>
							<div class="font-mono text-base">{fmtNum(r.val)}</div>
							{#if baseline}
								<div class="font-mono text-xs {deltaClass(r.val, r.base)}">{delta(r.val, r.base)}</div>
							{/if}
						</div>
					{/each}
				</div>
			</section>

			<!-- Cognito -->
			<section class="rounded-lg border border-border bg-card p-4">
				<h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
					Cognito
				</h2>
				<div class="mb-3 grid grid-cols-2 gap-3 sm:grid-cols-4">
					{#each cogRows as r (r.label)}
						<div class="rounded border border-border/60 bg-background p-3">
							<div class="text-xs text-muted-foreground">{r.label}</div>
							<div class="font-mono text-base">{fmtNum(r.val)}</div>
							{#if baseline}
								<div class="font-mono text-xs {deltaClass(r.val, r.base)}">{delta(r.val, r.base)}</div>
							{/if}
						</div>
					{/each}
				</div>

				{#if current.cognito.per_pool.length > 0}
					<div class="overflow-x-auto rounded border border-border/60">
						<table class="w-full text-sm">
							<thead class="bg-muted/40 text-xs uppercase text-muted-foreground">
								<tr>
									<th class="px-3 py-2 text-left">Pool ID</th>
									<th class="px-3 py-2 text-right">Users</th>
									<th class="px-3 py-2 text-right">Groups</th>
									<th class="px-3 py-2 text-right">Clients</th>
									<th class="px-3 py-2 text-right">Auth events</th>
									<th class="px-3 py-2 text-right">Devices</th>
									<th class="px-3 py-2 text-right">Revoked tokens</th>
								</tr>
							</thead>
							<tbody>
								{#each current.cognito.per_pool as p (p.id)}
									<tr class="border-t border-border/40">
										<td class="px-3 py-2 font-mono text-xs">{p.id}</td>
										<td class="px-3 py-2 text-right font-mono">{fmtNum(p.users)}</td>
										<td class="px-3 py-2 text-right font-mono">{fmtNum(p.groups)}</td>
										<td class="px-3 py-2 text-right font-mono">{fmtNum(p.clients)}</td>
										<td class="px-3 py-2 text-right font-mono">{fmtNum(p.auth_events_total)}</td>
										<td class="px-3 py-2 text-right font-mono">{fmtNum(p.devices_total)}</td>
										<td class="px-3 py-2 text-right font-mono">{fmtNum(p.revoked_refresh_tokens_total)}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{/if}
			</section>

			<!-- Billing -->
			<section class="rounded-lg border border-border bg-card p-4">
				<h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
					Billing meter
				</h2>
				<div class="grid grid-cols-2 gap-3 sm:grid-cols-5">
					{#each billRows as r (r.label)}
						<div class="rounded border border-border/60 bg-background p-3">
							<div class="text-xs text-muted-foreground">{r.label}</div>
							<div class="font-mono text-base">{fmtNum(r.val)}</div>
							{#if baseline}
								<div class="font-mono text-xs {deltaClass(r.val, r.base)}">{delta(r.val, r.base)}</div>
							{/if}
						</div>
					{/each}
				</div>
			</section>

			<!-- SQLite-backed stores -->
			<section class="rounded-lg border border-border bg-card p-4">
				<h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
					SQLite-backed stores
				</h2>
				<div class="grid grid-cols-2 gap-3 sm:grid-cols-5">
					{#each sqlRows as r (r.label)}
						<div class="rounded border border-border/60 bg-background p-3">
							<div class="text-xs text-muted-foreground">{r.label}</div>
							<div class="font-mono text-base">{fmtNum(r.val)}</div>
							{#if baseline}
								<div class="font-mono text-xs {deltaClass(r.val, r.base)}">{delta(r.val, r.base)}</div>
							{/if}
						</div>
					{/each}
					<div class="rounded border border-border/60 bg-background p-3">
						<div class="text-xs text-muted-foreground">DynamoDB db file</div>
						<div class="font-mono text-base">{fmtBytes(current.sqlite.dynamodb_db_size_bytes)}</div>
						{#if baseline}
							<div
								class="font-mono text-xs {deltaClass(current.sqlite.dynamodb_db_size_bytes, baseline.sqlite.dynamodb_db_size_bytes)}"
							>
								{deltaBytes(current.sqlite.dynamodb_db_size_bytes, baseline.sqlite.dynamodb_db_size_bytes)}
							</div>
						{/if}
					</div>
				</div>
			</section>

			{#if baseline}
				<div class="flex items-center justify-between rounded border border-orange-500/40 bg-orange-500/5 px-3 py-2 text-xs">
					<span>
						<Badge variant="outline" class="mr-2">baseline</Badge>
						Captured {new Date(baseline.captured_at * 1000).toLocaleTimeString()} —
						deltas shown next to current values.
					</span>
					<button
						type="button"
						class="text-orange-300 underline hover:text-orange-200"
						onclick={clearBaseline}
					>
						clear
					</button>
				</div>
			{/if}
		{/if}
	</div>
</ServicePage>
