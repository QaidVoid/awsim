<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { fetchHealth, fetchServices, fetchConfig, fetchStats } from '$lib/api';

	let health = $state<any>(null);
	let services = $state<any[]>([]);
	let config = $state<any>(null);
	let stats = $state<any>(null);
	let interval: ReturnType<typeof setInterval>;

	onMount(async () => {
		[health, { services }, config] = await Promise.all([
			fetchHealth(),
			fetchServices(),
			fetchConfig(),
		]);

		const pollStats = async () => {
			try {
				stats = await fetchStats();
			} catch {}
		};
		pollStats();
		interval = setInterval(pollStats, 2000);
	});

	onDestroy(() => {
		if (interval) clearInterval(interval);
	});
</script>

<div class="p-6">
	<h1 class="text-2xl font-bold mb-6">Dashboard</h1>

	<!-- Metrics grid: 2 cols on mobile, 3 cols on md+ -->
	<div class="grid grid-cols-2 md:grid-cols-3 gap-4 mb-8">
		<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4">
			<div class="text-zinc-500 text-sm">Status</div>
			<div class="text-2xl font-bold mt-1">
				{#if health}
					<span class="text-green-400">Online</span>
				{:else}
					<span class="text-zinc-600">Loading...</span>
				{/if}
			</div>
		</div>
		<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4">
			<div class="text-zinc-500 text-sm">Region</div>
			<div class="text-2xl font-bold mt-1">{config?.region ?? '...'}</div>
		</div>
		<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4">
			<div class="text-zinc-500 text-sm">Services</div>
			<div class="text-2xl font-bold mt-1">{services.length || '...'}</div>
		</div>
		<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4">
			<div class="text-zinc-500 text-sm">Uptime</div>
			<div class="text-2xl font-bold mt-1 font-mono">
				{stats?.uptimeFormatted ?? '...'}
			</div>
		</div>
		<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4">
			<div class="text-zinc-500 text-sm">Total Requests</div>
			<div class="text-2xl font-bold mt-1 font-mono">
				{stats?.totalRequests ?? '...'}
			</div>
		</div>
		<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4">
			<div class="text-zinc-500 text-sm">Requests / sec</div>
			<div class="text-2xl font-bold mt-1 font-mono">
				{stats?.requestsPerSecond ?? '...'}
			</div>
		</div>
	</div>

	<!-- Service grid -->
	<h2 class="text-lg font-semibold mb-3 text-zinc-300">Registered Services</h2>
	<div class="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-5 gap-3">
		{#each services as svc}
			<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-3 hover:border-zinc-600 transition-colors">
				<div class="font-medium text-sm">{svc.name}</div>
				<div class="text-xs text-zinc-500 mt-1">{svc.protocol}</div>
			</div>
		{/each}
	</div>

	<!-- Account info -->
	{#if config}
		<div class="mt-8 bg-zinc-900 rounded-lg border border-zinc-800 p-4">
			<h2 class="text-lg font-semibold mb-2 text-zinc-300">Configuration</h2>
			<div class="grid grid-cols-2 gap-2 text-sm">
				<div class="text-zinc-500">Account ID</div>
				<div class="font-mono">{config.accountId}</div>
				<div class="text-zinc-500">Region</div>
				<div class="font-mono">{config.region}</div>
			</div>
		</div>
	{/if}
</div>
