<script lang="ts">
	/**
	 * Dashboard home — composes the live request stream, KPI strip,
	 * service status list and insights panel using the shared
	 * `dashboardState` (SSE) plus polled storage / stats / config.
	 */
	import { onDestroy, onMount } from 'svelte';
	import { fetchConfig, fetchStats, fetchStorage } from '$lib/api';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import KpiCard from '$lib/components/dashboard/kpi-card.svelte';
	import RequestStream from '$lib/components/dashboard/request-stream.svelte';
	import ServiceStatusList from '$lib/components/dashboard/service-status-list.svelte';
	import InsightsPanel from '$lib/components/dashboard/insights-panel.svelte';
	import { dashboardState } from '$lib/dashboard-state.svelte';
	import { bytesHuman, durationHuman } from '$lib/format';
	import type { StoragePayload } from '$lib/events';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Activity from '@lucide/svelte/icons/activity';
	import Gauge from '@lucide/svelte/icons/gauge';
	import HardDrive from '@lucide/svelte/icons/hard-drive';
	import Clock from '@lucide/svelte/icons/clock';

	interface Stats {
		uptime?: number;
		uptimeFormatted?: string;
		totalRequests?: number;
		requestsPerSecond?: number | string;
		services?: number;
	}
	interface Config {
		region?: string;
		accountId?: string;
		services?: number;
	}

	let stats = $state<Stats | null>(null);
	let config = $state<Config | null>(null);
	let storage = $state<StoragePayload | null>(null);
	let pollTimer: ReturnType<typeof setInterval> | undefined;
	let refreshing = $state(false);
	let lastError = $state<string | null>(null);
	let lastUpdated = $state<number | null>(null);

	// Tick clock so the live RPS KPI re-renders smoothly.
	let now = $state(Date.now() / 1000);
	$effect(() => {
		const id = setInterval(() => (now = Date.now() / 1000), 1000);
		return () => clearInterval(id);
	});

	const liveRps = $derived(dashboardState.rps(5, now));
	const totalRequests = $derived(stats?.totalRequests ?? null);
	const totalDisk = $derived(storage?.total_size_bytes ?? null);
	const persistenceOn = $derived(Boolean(storage?.data_dir));
	const serviceCount = $derived(stats?.services ?? config?.services ?? 0);

	async function refresh() {
		refreshing = true;
		try {
			const [s, c, st] = await Promise.all([
				fetchStats() as Promise<Stats>,
				fetchConfig() as Promise<Config>,
				fetchStorage().catch(() => null as StoragePayload | null),
			]);
			stats = s;
			config = c;
			storage = st;
			lastError = null;
			lastUpdated = Date.now() / 1000;
		} catch (err) {
			lastError = err instanceof Error ? err.message : 'Failed to refresh';
		} finally {
			refreshing = false;
		}
	}

	onMount(() => {
		dashboardState.connect();
		refresh();
		pollTimer = setInterval(refresh, 5000);
	});

	onDestroy(() => {
		if (pollTimer) clearInterval(pollTimer);
		dashboardState.disconnect();
	});
</script>

<svelte:head>
	<title>AWSim · Dashboard</title>
</svelte:head>

<div class="space-y-4">
	<!-- 1. Header strip -->
	<header class="flex flex-wrap items-end justify-between gap-3">
		<div>
			<h1 class="text-2xl font-semibold tracking-tight">Dashboard</h1>
			<p class="mt-0.5 text-xs text-muted-foreground">
				{serviceCount || '—'} services · {config?.region ?? 'us-east-1'}
			</p>
		</div>
		<div class="flex items-center gap-2">
			{#if lastError}
				<span class="text-[11px] text-rose-400" title={lastError}>offline</span>
			{:else if lastUpdated}
				<span class="hidden text-[11px] text-muted-foreground sm:inline">
					updated {Math.max(0, Math.floor(now - lastUpdated))}s ago
				</span>
			{/if}
			<Badge variant="outline" class="gap-1.5">
				<span
					class="size-1.5 rounded-full"
					class:bg-emerald-400={persistenceOn}
					class:bg-muted-foreground={!persistenceOn}
				></span>
				<span class="text-[11px]">
					Persistence: {persistenceOn ? 'on' : 'off'}
				</span>
			</Badge>
			<Button
				size="sm"
				variant="outline"
				onclick={refresh}
				disabled={refreshing}
				class="h-7 gap-1.5 px-2.5"
			>
				<RefreshCw class={`size-3.5 ${refreshing ? 'animate-spin' : ''}`} />
				<span class="text-xs">Refresh</span>
			</Button>
		</div>
	</header>

	<!-- 2. KPI strip -->
	<section class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
		<KpiCard
			label="Total requests"
			value={totalRequests === null ? null : totalRequests.toLocaleString()}
			secondary="since boot"
			icon={Activity}
			loading={stats === null}
		/>
		<KpiCard
			label="Live RPS"
			value={liveRps.toFixed(liveRps >= 10 ? 0 : 1)}
			secondary="trailing 5s window"
			icon={Gauge}
			mono
			accent={liveRps > 0 ? 'emerald' : 'default'}
		/>
		<KpiCard
			label="Disk usage"
			value={totalDisk === null ? null : bytesHuman(totalDisk)}
			secondary={persistenceOn ? 'across blob stores' : 'persistence disabled'}
			icon={HardDrive}
			loading={storage === null}
			accent={persistenceOn ? 'sky' : 'default'}
		/>
		<KpiCard
			label="Uptime"
			value={stats?.uptime !== undefined ? durationHuman(stats.uptime) : null}
			secondary={config?.accountId ? `account ${config.accountId}` : undefined}
			icon={Clock}
			mono
			loading={stats === null}
		/>
	</section>

	<!-- 3. Two-column main row -->
	<section class="grid grid-cols-1 gap-3 lg:grid-cols-3">
		<div class="lg:col-span-2">
			<RequestStream />
		</div>
		<div>
			<ServiceStatusList {storage} />
		</div>
	</section>

	<!-- 4. Insights row -->
	<InsightsPanel {storage} {config} />
</div>
