<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { fetchConfig, fetchHealth, fetchServices, fetchStats } from '$lib/api';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { findService, SERVICES } from '$lib/services-catalog';
	import { cn } from '$lib/utils';
	import ArrowRight from '@lucide/svelte/icons/arrow-right';
	import Activity from '@lucide/svelte/icons/activity';
	import Sparkles from '@lucide/svelte/icons/sparkles';

	interface Health {
		status?: string;
	}
	interface ServicesResponse {
		services?: Array<{ name: string; protocol?: string }>;
	}
	interface Config {
		region?: string;
		accountId?: string;
	}
	interface Stats {
		uptimeFormatted?: string;
		totalRequests?: number;
		requestsPerSecond?: number | string;
	}

	let health = $state<Health | null>(null);
	let services = $state<ServicesResponse['services']>([]);
	let config = $state<Config | null>(null);
	let stats = $state<Stats | null>(null);
	let interval: ReturnType<typeof setInterval> | undefined;

	// Quick links — the services we expect users to hit most often.
	const QUICK_LINK_IDS = ['s3', 'lambda', 'dynamodb', 'sqs', 'iam', 'logs'] as const;
	const quickLinks = QUICK_LINK_IDS.map((id) => findService(id)).filter(
		(s): s is NonNullable<ReturnType<typeof findService>> => Boolean(s)
	);

	onMount(async () => {
		try {
			const [h, s, c] = await Promise.all([
				fetchHealth() as Promise<Health>,
				fetchServices() as Promise<ServicesResponse>,
				fetchConfig() as Promise<Config>,
			]);
			health = h;
			services = s.services ?? [];
			config = c;
		} catch {
			/* ignore — endpoints may be unavailable in dev */
		}

		const pollStats = async () => {
			try {
				stats = await fetchStats();
			} catch {
				/* ignore */
			}
		};
		pollStats();
		interval = setInterval(pollStats, 2000);
	});

	onDestroy(() => {
		if (interval) clearInterval(interval);
	});

	function fmtNumber(value: number | string | undefined): string {
		if (value === undefined || value === null) return '—';
		if (typeof value === 'number') return value.toLocaleString();
		return value;
	}
</script>

<svelte:head>
	<title>AWSim · Dashboard</title>
</svelte:head>

<div class="space-y-6">
	<header class="flex flex-wrap items-end justify-between gap-3 border-b border-border pb-4">
		<div>
			<h1 class="text-2xl font-semibold tracking-tight">Dashboard</h1>
			<p class="mt-1 text-sm text-muted-foreground">
				Local AWS emulator overview. A richer dashboard arrives in the next phase.
			</p>
		</div>
		<Badge variant="outline" class="gap-1.5">
			{#if health}
				<span class="size-1.5 rounded-full bg-emerald-400"></span>
				<span class="text-xs">Online</span>
			{:else}
				<span class="size-1.5 rounded-full bg-muted-foreground"></span>
				<span class="text-xs">Connecting…</span>
			{/if}
		</Badge>
	</header>

	<!-- Stat strip — dense, monospaced metrics. -->
	<section class="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-6">
		{#each [
			{ label: 'Region', value: config?.region, mono: true },
			{ label: 'Account', value: config?.accountId, mono: true },
			{ label: 'Services', value: services?.length ? String(services.length) : undefined },
			{ label: 'Uptime', value: stats?.uptimeFormatted, mono: true },
			{ label: 'Requests', value: fmtNumber(stats?.totalRequests) },
			{ label: 'Req / sec', value: fmtNumber(stats?.requestsPerSecond), mono: true },
		] as cell (cell.label)}
			<div class="rounded-md border border-border bg-card p-3">
				<div class="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
					{cell.label}
				</div>
				{#if cell.value === undefined || cell.value === '—'}
					<Skeleton class="mt-2 h-5 w-16" />
				{:else}
					<div
						class={cn('mt-1 text-lg font-semibold', cell.mono && 'font-mono')}
					>
						{cell.value}
					</div>
				{/if}
			</div>
		{/each}
	</section>

	<!-- Future-dashboard placeholder grid. -->
	<section
		class="grid grid-cols-1 gap-4 lg:grid-cols-3"
		aria-label="Dashboard placeholders"
	>
		<div
			class="col-span-1 flex h-[260px] flex-col items-center justify-center rounded-lg border border-dashed border-border bg-card/40 p-6 text-center lg:col-span-2"
		>
			<div
				class="mb-3 flex size-10 items-center justify-center rounded-md bg-muted text-muted-foreground"
			>
				<Sparkles class="size-5" />
			</div>
			<div class="text-sm font-medium">Dashboard coming in next phase</div>
			<div class="mt-1 max-w-md text-xs text-muted-foreground">
				This space will render charts of request rate, top services, blob
				storage usage, recent IAM denials and more. Hit
				<kbd
					class="mx-1 inline-flex h-5 items-center gap-0.5 rounded border border-border bg-muted px-1.5 font-mono text-[10px]"
					>⌘K</kbd
				>
				to jump straight into a service in the meantime.
			</div>
		</div>

		<div class="rounded-lg border border-border bg-card p-4">
			<div class="mb-3 flex items-center gap-2 text-sm font-semibold">
				<Activity class="size-4 text-muted-foreground" />
				<span>Quick links</span>
			</div>
			<ul class="space-y-1">
				{#each quickLinks as svc (svc.id)}
					<li>
						<Button
							variant="ghost"
							size="sm"
							href={svc.href}
							class="w-full justify-start gap-2 px-2"
						>
							<svc.icon class="size-4 text-muted-foreground" />
							<span>{svc.name}</span>
							<ArrowRight class="ml-auto size-3.5 text-muted-foreground" />
						</Button>
					</li>
				{/each}
			</ul>
		</div>
	</section>

	<!-- Registered services preview — uses the services-catalog so the order
		 matches the sidebar. -->
	<section>
		<div class="mb-2 flex items-baseline justify-between">
			<h2 class="text-sm font-semibold tracking-tight">Registered services</h2>
			<span class="font-mono text-xs text-muted-foreground"
				>{services?.length ?? 0} backends</span
			>
		</div>
		<div
			class="grid grid-cols-2 gap-2 sm:grid-cols-3 md:grid-cols-4 xl:grid-cols-6"
		>
			{#each SERVICES.slice(0, 24) as svc (svc.id)}
				<a
					href={svc.href}
					class="group flex items-center gap-2 rounded-md border border-border bg-card p-2.5 transition-all duration-100 hover:border-primary/40 hover:bg-card/80"
				>
					<div
						class="flex size-7 items-center justify-center rounded bg-muted text-muted-foreground group-hover:text-foreground"
					>
						<svc.icon class="size-3.5" />
					</div>
					<div class="min-w-0 flex-1">
						<div class="truncate text-sm font-medium">{svc.name}</div>
						<div class="truncate font-mono text-[10px] text-muted-foreground">
							{svc.href}
						</div>
					</div>
				</a>
			{/each}
		</div>
	</section>
</div>

