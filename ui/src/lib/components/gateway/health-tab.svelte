<script lang="ts">
	// Live status board for the configured backends. Reads the
	// process-lifetime HealthRegistry behind /_awsim/gateway/health,
	// auto-refreshes every 5s, lets the user trigger a one-off probe
	// per backend.
	import { onMount, onDestroy } from 'svelte';
	import {
		getGatewayHealth,
		recheckGatewayBackend,
		type BackendHealth,
		type BackendStatus,
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
	import ZapIcon from '@lucide/svelte/icons/zap';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import { toast } from 'svelte-sonner';

	const POLL_MS = 5000;

	let backends = $state<BackendHealth[]>([]);
	let loading = $state(true);
	let checking = $state<Record<string, boolean>>({});
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
			const res = await getGatewayHealth();
			backends = res.backends;
		} catch (e) {
			// Silent on the auto-poll path; surface only the manual
			// failure (otherwise a brief network glitch spams a toast
			// every 5 seconds).
			if (loading) {
				toast.error(e instanceof Error ? e.message : 'Failed to load health');
			}
		} finally {
			loading = false;
		}
	}

	async function recheck(name: string) {
		checking[name] = true;
		try {
			const res = await recheckGatewayBackend(name);
			// Splice the updated record back into the local list so
			// the UI updates without waiting for the next poll tick.
			const idx = backends.findIndex((b) => b.backend === name);
			if (idx >= 0) {
				backends[idx] = res.backend;
				backends = backends;
			} else {
				backends = [...backends, res.backend].sort((a, b) =>
					a.backend.localeCompare(b.backend),
				);
			}
			if (res.result.error) {
				toast.error(`'${name}' check failed: ${res.result.error}`);
			} else {
				toast.success(`'${name}' is up (${res.result.latency_ms}ms)`);
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Recheck failed');
		} finally {
			checking[name] = false;
		}
	}

	function statusBadge(status: BackendStatus): {
		variant: 'default' | 'secondary' | 'destructive' | 'outline';
		label: string;
	} {
		switch (status) {
			case 'healthy':
				return { variant: 'default', label: 'Healthy' };
			case 'degraded':
				return { variant: 'secondary', label: 'Degraded' };
			case 'down':
				return { variant: 'destructive', label: 'Down' };
			case 'unknown':
				return { variant: 'outline', label: 'Unknown' };
		}
	}

	function relativeAgo(iso: string | null): string {
		if (!iso) return 'never';
		const ms = Date.now() - new Date(iso).getTime();
		if (ms < 1000) return 'just now';
		if (ms < 60_000) return `${Math.floor(ms / 1000)}s ago`;
		if (ms < 3_600_000) return `${Math.floor(ms / 60_000)}m ago`;
		return `${Math.floor(ms / 3_600_000)}h ago`;
	}

	// Compact pass/fail sparkline of the last ~30 checks. Pure CSS;
	// height encodes nothing (constant), color encodes outcome.
	function historySpark(b: BackendHealth, max = 30): { ok: boolean }[] {
		const slice = b.history.slice(-max);
		return slice.map((r) => ({ ok: r.error === null }));
	}
</script>

<div class="space-y-4 p-4">
	<section class="rounded-lg border bg-card p-4 text-sm">
		<div class="flex items-start gap-3">
			<ActivityIcon class="mt-0.5 h-5 w-5 shrink-0 text-muted-foreground" />
			<div class="space-y-1">
				<p class="font-semibold">Backend health</p>
				<p class="text-muted-foreground">
					A background poller hits each backend's <code>/models</code> endpoint every 30 seconds.
					Two consecutive failures flip a backend to <strong>Down</strong>; the alias resolver then
					skips it. One success flips it back to Healthy. Click Recheck for an on-demand probe.
				</p>
			</div>
		</div>
	</section>

	<header class="flex items-center justify-between">
		<div>
			<h2 class="text-lg font-semibold">Status</h2>
			<p class="text-sm text-muted-foreground">
				{backends.length} backend{backends.length === 1 ? '' : 's'} · auto-refresh every 5s
			</p>
		</div>
		<Button variant="ghost" size="sm" onclick={reload} disabled={loading}>
			<RefreshCwIcon class={loading ? 'h-4 w-4 animate-spin' : 'h-4 w-4'} />
			<span class="ml-2">Refresh</span>
		</Button>
	</header>

	{#if loading && backends.length === 0}
		<EmptyState icon={ActivityIcon} title="Loading health…" />
	{:else if backends.length === 0}
		<EmptyState
			icon={ActivityIcon}
			title="No backends to probe"
			description="The poller starts pinging /models once you add at least one backend. The first tick may take up to 30 seconds."
		/>
	{:else}
		<div class="rounded-lg border bg-card">
			<Table>
				<TableHeader>
					<TableRow>
						<TableHead>Backend</TableHead>
						<TableHead>Status</TableHead>
						<TableHead>Latency</TableHead>
						<TableHead>Last check</TableHead>
						<TableHead>History</TableHead>
						<TableHead>Last error</TableHead>
						<TableHead class="text-right">Actions</TableHead>
					</TableRow>
				</TableHeader>
				<TableBody>
					{#each backends as b (b.backend)}
						{@const sb = statusBadge(b.status)}
						{@const ticks = historySpark(b)}
						<TableRow>
							<TableCell class="font-mono text-sm">{b.backend}</TableCell>
							<TableCell>
								<Badge variant={sb.variant} class="text-[10px] uppercase">
									{sb.label}
								</Badge>
								{#if b.consecutiveFailures > 1}
									<span class="ml-1 text-xs text-muted-foreground">
										×{b.consecutiveFailures}
									</span>
								{/if}
							</TableCell>
							<TableCell class="font-mono text-xs">
								{b.lastLatencyMs !== null ? `${b.lastLatencyMs}ms` : '—'}
							</TableCell>
							<TableCell class="text-xs text-muted-foreground">
								{relativeAgo(b.lastCheckedAt)}
							</TableCell>
							<TableCell>
								<div class="flex items-end gap-0.5">
									{#each ticks as t, i (i)}
										<span
											class={'inline-block h-3 w-1 rounded-sm ' +
												(t.ok ? 'bg-emerald-500/70' : 'bg-rose-500/70')}
											title={t.ok ? 'ok' : 'failed'}
										></span>
									{/each}
									{#if ticks.length === 0}
										<span class="text-xs text-muted-foreground">no history</span>
									{/if}
								</div>
							</TableCell>
							<TableCell class="max-w-xs truncate text-xs text-muted-foreground" title={b.lastError ?? ''}>
								{b.lastError ?? '—'}
							</TableCell>
							<TableCell class="text-right">
								<Button
									variant="ghost"
									size="sm"
									onclick={() => recheck(b.backend)}
									disabled={checking[b.backend]}
								>
									<ZapIcon class={checking[b.backend] ? 'h-4 w-4 animate-pulse' : 'h-4 w-4'} />
									<span class="ml-1">Recheck</span>
								</Button>
							</TableCell>
						</TableRow>
					{/each}
				</TableBody>
			</Table>
		</div>
	{/if}
</div>
