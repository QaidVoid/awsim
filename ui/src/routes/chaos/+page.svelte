<script lang="ts">
	/**
	 * Chaos engine dashboard — preset cards, rules table with kill-switch
	 * toggles, manual rule add, recent-injections sparkline. Auto-refreshes
	 * every 3s while the tab is open.
	 */
	import { onDestroy, onMount } from 'svelte';
	import { ServicePage, EmptyState } from '$lib/components/service';
	import { Button, buttonVariants } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Switch } from '$lib/components/ui/switch';
	import {
		Card,
		CardContent,
		CardDescription,
		CardHeader,
		CardTitle
	} from '$lib/components/ui/card';
	import {
		Table,
		TableBody,
		TableCell,
		TableHead,
		TableHeader,
		TableRow
	} from '$lib/components/ui/table';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle,
		DialogTrigger
	} from '$lib/components/ui/dialog';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import FlameIcon from '@lucide/svelte/icons/flame';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import { toast } from 'svelte-sonner';
	import {
		fetchChaosRules,
		fetchChaosStats,
		fetchChaosPresets,
		applyChaosPreset,
		addChaosRule,
		setChaosRuleEnabled,
		removeChaosRule,
		clearChaosRules,
		type ChaosRule,
		type ChaosRecentInjection,
		type ChaosPresetInfo
	} from '$lib/api';

	const REFRESH_MS = 3_000;

	let rules = $state<ChaosRule[]>([]);
	let totalInjections = $state(0);
	let recent = $state<ChaosRecentInjection[]>([]);
	let presets = $state<ChaosPresetInfo[]>([]);
	let lastUpdated = $state<Date | null>(null);
	let loading = $state(false);
	let timer: ReturnType<typeof setInterval> | null = null;

	// Add-rule dialog state.
	let addOpen = $state(false);
	let clearOpen = $state(false);
	let clearBusy = $state(false);
	let formService = $state('*');
	let formOperation = $state('*');
	let formProbability = $state(1.0);
	let formErrorStatus = $state('503');
	let formErrorCode = $state('');
	let formErrorMessage = $state('');
	let formLatencyMin = $state('');
	let formLatencyMax = $state('');
	let formLabel = $state('');
	let formStartInSecs = $state('');
	let formTtlSecs = $state('');
	let formFlap = $state('');

	async function refresh() {
		loading = true;
		try {
			const [r, s] = await Promise.all([fetchChaosRules(), fetchChaosStats()]);
			rules = r.rules ?? [];
			totalInjections = s.total_injections ?? r.total_injections ?? 0;
			recent = s.recent ?? [];
			lastUpdated = new Date();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to refresh chaos');
		} finally {
			loading = false;
		}
	}

	async function loadPresets() {
		try {
			const r = await fetchChaosPresets();
			presets = r.presets ?? [];
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load presets');
		}
	}

	onMount(() => {
		refresh();
		loadPresets();
		timer = setInterval(refresh, REFRESH_MS);
	});

	onDestroy(() => {
		if (timer) clearInterval(timer);
	});

	async function applyPreset(name: string) {
		try {
			const r = await applyChaosPreset(name);
			toast.success(`Applied ${name} (+${r.rule_ids.length} rule(s))`);
			await refresh();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to apply preset');
		}
	}

	async function toggleRule(id: string, enabled: boolean) {
		try {
			await setChaosRuleEnabled(id, enabled);
			await refresh();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to toggle rule');
		}
	}

	async function deleteRule(id: string) {
		try {
			await removeChaosRule(id);
			toast.success('Rule removed');
			await refresh();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to delete rule');
		}
	}

	function clearAll() {
		clearOpen = true;
	}

	async function confirmClearAll() {
		clearBusy = true;
		try {
			await clearChaosRules();
			toast.success('All rules cleared');
			clearOpen = false;
			await refresh();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to clear');
		} finally {
			clearBusy = false;
		}
	}

	function resetForm() {
		formService = '*';
		formOperation = '*';
		formProbability = 1.0;
		formErrorStatus = '503';
		formErrorCode = '';
		formErrorMessage = '';
		formLatencyMin = '';
		formLatencyMax = '';
		formLabel = '';
		formStartInSecs = '';
		formTtlSecs = '';
		formFlap = '';
	}

	function buildScheduleFromForm(): { schedule: Record<string, unknown> } | { error: string } | null {
		const hasStart = formStartInSecs.trim() !== '';
		const hasTtl = formTtlSecs.trim() !== '';
		const hasFlap = formFlap.trim() !== '';
		if (!hasStart && !hasTtl && !hasFlap) return null;

		const now = Math.floor(Date.now() / 1000);
		let startTs: number | undefined;
		let endTs: number | undefined;
		if (hasStart) {
			const n = parseInt(formStartInSecs, 10);
			if (Number.isNaN(n) || n < 0) return { error: 'Invalid start-in-secs.' };
			startTs = now + n;
		}
		if (hasTtl) {
			const n = parseInt(formTtlSecs, 10);
			if (Number.isNaN(n) || n <= 0) return { error: 'Invalid ttl-secs.' };
			endTs = (startTs ?? now) + n;
		}
		const window =
			startTs !== undefined || endTs !== undefined ? { start_ts: startTs, end_ts: endTs } : undefined;

		let flap: { period_secs: number; active_secs: number; anchor_ts: number } | undefined;
		if (hasFlap) {
			const m = formFlap.match(/^(\d+)\s*\/\s*(\d+)$/);
			if (!m) return { error: 'Flap must be ACTIVE/PERIOD (e.g. 30/60).' };
			const active = parseInt(m[1]!, 10);
			const period = parseInt(m[2]!, 10);
			if (active <= 0 || period <= 0 || active > period)
				return { error: 'Flap: 0 < active <= period.' };
			flap = { active_secs: active, period_secs: period, anchor_ts: startTs ?? now };
		}

		return { schedule: { window, flap } };
	}

	async function submitAdd() {
		// Build the effect from the form: error and/or latency.
		const hasError = formErrorCode.trim() !== '';
		const hasLatency = formLatencyMin.trim() !== '' || formLatencyMax.trim() !== '';
		if (!hasError && !hasLatency) {
			toast.error('Specify at least an error code or a latency range.');
			return;
		}

		// Latency parsing — if only min is given, treat as fixed.
		let latency: { min_ms: number; max_ms: number } | null = null;
		if (hasLatency) {
			const min = parseInt(formLatencyMin || formLatencyMax, 10);
			const max = parseInt(formLatencyMax || formLatencyMin, 10);
			if (Number.isNaN(min) || Number.isNaN(max) || min < 0 || max < min) {
				toast.error('Invalid latency range.');
				return;
			}
			latency = { min_ms: min, max_ms: max };
		}

		let error: { status: number; code: string; message: string } | null = null;
		if (hasError) {
			const status = parseInt(formErrorStatus, 10);
			if (Number.isNaN(status) || status < 100 || status > 599) {
				toast.error('Invalid HTTP status.');
				return;
			}
			error = {
				status,
				code: formErrorCode.trim(),
				message: formErrorMessage.trim() || `synthetic ${formErrorCode.trim()}`
			};
		}

		let effect: Record<string, unknown>;
		if (latency && error) {
			effect = { kind: 'both', latency, error };
		} else if (latency) {
			effect = { kind: 'latency', ...latency };
		} else if (error) {
			effect = { kind: 'error', ...error };
		} else {
			return; // unreachable per check above
		}

		const service =
			formService === '*' ? { kind: 'any' } : { kind: 'exact', value: formService.trim() };
		const operation =
			formOperation === '*'
				? { kind: 'any' }
				: { kind: 'exact', value: formOperation.trim() };

		const sched = buildScheduleFromForm();
		if (sched && 'error' in sched) {
			toast.error(sched.error);
			return;
		}

		try {
			await addChaosRule({
				service: service as ChaosRule['service'],
				operation: operation as ChaosRule['operation'],
				probability: formProbability,
				effect: effect as ChaosRule['effect'],
				label: formLabel.trim() || null,
				schedule: (sched?.schedule ?? null) as ChaosRule['schedule']
			});
			toast.success('Rule added');
			addOpen = false;
			resetForm();
			await refresh();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to add rule');
		}
	}

	function formatService(m: ChaosRule['service']): string {
		return m.kind === 'any' ? '*' : m.value;
	}

	function formatOperation(m: ChaosRule['operation']): string {
		return m.kind === 'any' ? '*' : m.value;
	}

	function formatEffect(e: ChaosRule['effect']): string {
		if (e.kind === 'error') return `[${e.status}] ${e.code}`;
		if (e.kind === 'latency')
			return e.min_ms === e.max_ms ? `+${e.min_ms}ms` : `+${e.min_ms}-${e.max_ms}ms`;
		// Both
		const lat =
			e.latency.min_ms === e.latency.max_ms
				? `+${e.latency.min_ms}ms`
				: `+${e.latency.min_ms}-${e.latency.max_ms}ms`;
		return `${lat} then [${e.error.status}] ${e.error.code}`;
	}

	function describeSchedule(s: ChaosRule['schedule']): string | null {
		if (!s) return null;
		const now = Math.floor(Date.now() / 1000);
		const parts: string[] = [];
		if (s.window) {
			const { start_ts, end_ts } = s.window;
			if (start_ts !== undefined && end_ts !== undefined) {
				parts.push(`window ${signedDelta(start_ts, now)} → ${signedDelta(end_ts, now)}`);
			} else if (start_ts !== undefined) {
				parts.push(`starts ${signedDelta(start_ts, now)}`);
			} else if (end_ts !== undefined) {
				parts.push(`ends ${signedDelta(end_ts, now)}`);
			}
		}
		if (s.flap) {
			parts.push(
				`flap ${s.flap.active_secs}s on / ${s.flap.period_secs - s.flap.active_secs}s off`
			);
		}
		return parts.length > 0 ? parts.join(', ') : null;
	}

	function signedDelta(target: number, now: number): string {
		return target >= now ? `in ${target - now}s` : `${now - target}s ago`;
	}

	function formatTs(ts: number): string {
		const d = new Date(ts * 1000);
		const diff = Date.now() - d.getTime();
		if (diff < 60_000) return `${Math.floor(diff / 1000)}s ago`;
		if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
		return d.toLocaleTimeString();
	}

	// Sparkline data — bin recent injections into 30 buckets covering the
	// past 5 min so the user gets a quick visual on how aggressive things
	// have been.
	const SPARK_BUCKETS = 30;
	const SPARK_WINDOW_S = 300;
	let spark = $derived.by(() => {
		const now = Math.floor(Date.now() / 1000);
		const bucketSize = SPARK_WINDOW_S / SPARK_BUCKETS;
		const buckets = new Array(SPARK_BUCKETS).fill(0);
		for (const inj of recent) {
			const age = now - inj.ts;
			if (age < 0 || age >= SPARK_WINDOW_S) continue;
			const idx = Math.min(SPARK_BUCKETS - 1, Math.floor((SPARK_WINDOW_S - age) / bucketSize));
			buckets[idx]++;
		}
		return buckets;
	});
	let sparkMax = $derived(Math.max(1, ...spark));
</script>

<svelte:head>
	<title>AWSim · Chaos</title>
</svelte:head>

<ServicePage
	title="Chaos engine"
	description="Inject synthetic AWS errors and latency into the gateway. Useful for testing retry/backoff, circuit breakers, and graceful degradation."
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={refresh} disabled={loading}>
			<RefreshCwIcon class="mr-1 h-4 w-4" />
			Refresh
		</Button>
		<Dialog bind:open={addOpen}>
			<DialogTrigger class={buttonVariants({ size: 'sm' })}>
				<PlusIcon class="mr-1 h-4 w-4" />
				Add rule
			</DialogTrigger>
			<DialogContent class="sm:max-w-[480px]">
				<DialogHeader>
					<DialogTitle>Add chaos rule</DialogTitle>
					<DialogDescription>
						Specify an error effect, a latency effect, or both. Use <code>*</code> as a
						wildcard for service or operation.
					</DialogDescription>
				</DialogHeader>
				<div class="space-y-3">
					<div class="grid grid-cols-2 gap-3">
						<div>
							<Label for="svc">Service</Label>
							<Input id="svc" bind:value={formService} placeholder="s3 or *" />
						</div>
						<div>
							<Label for="op">Operation</Label>
							<Input
								id="op"
								bind:value={formOperation}
								placeholder="PutObject or *"
							/>
						</div>
					</div>
					<div>
						<Label for="prob">Probability ({formProbability.toFixed(2)})</Label>
						<Input
							id="prob"
							type="number"
							min="0"
							max="1"
							step="0.05"
							bind:value={formProbability}
						/>
					</div>
					<div class="rounded border border-border p-3">
						<div class="mb-2 text-sm font-medium">Error (optional)</div>
						<div class="grid grid-cols-3 gap-2">
							<div>
								<Label for="status">Status</Label>
								<Input id="status" bind:value={formErrorStatus} />
							</div>
							<div class="col-span-2">
								<Label for="code">AWS code</Label>
								<Input
									id="code"
									bind:value={formErrorCode}
									placeholder="SlowDown"
								/>
							</div>
						</div>
						<div class="mt-2">
							<Label for="msg">Message</Label>
							<Input
								id="msg"
								bind:value={formErrorMessage}
								placeholder="please retry"
							/>
						</div>
					</div>
					<div class="rounded border border-border p-3">
						<div class="mb-2 text-sm font-medium">Latency (optional, ms)</div>
						<div class="grid grid-cols-2 gap-2">
							<div>
								<Label for="lat-min">Min</Label>
								<Input id="lat-min" bind:value={formLatencyMin} placeholder="100" />
							</div>
							<div>
								<Label for="lat-max">Max</Label>
								<Input id="lat-max" bind:value={formLatencyMax} placeholder="500" />
							</div>
						</div>
					</div>
					<div class="rounded border border-border p-3">
						<div class="mb-2 text-sm font-medium">Schedule (optional)</div>
						<div class="grid grid-cols-2 gap-2">
							<div>
								<Label for="start-in">Start in (s)</Label>
								<Input
									id="start-in"
									bind:value={formStartInSecs}
									placeholder="0"
								/>
							</div>
							<div>
								<Label for="ttl">TTL (s)</Label>
								<Input id="ttl" bind:value={formTtlSecs} placeholder="600" />
							</div>
						</div>
						<div class="mt-2">
							<Label for="flap">Flap (active/period)</Label>
							<Input id="flap" bind:value={formFlap} placeholder="30/60" />
						</div>
					</div>
					<div>
						<Label for="label">Label</Label>
						<Input
							id="label"
							bind:value={formLabel}
							placeholder="e.g. flaky-deploy-2026-04"
						/>
					</div>
				</div>
				<DialogFooter>
					<Button variant="outline" onclick={() => (addOpen = false)}>Cancel</Button>
					<Button onclick={submitAdd}>Add rule</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
		<Button variant="outline" size="sm" onclick={clearAll} disabled={rules.length === 0}>
			<Trash2Icon class="mr-1 h-4 w-4" />
			Clear all
		</Button>
	{/snippet}

	<div class="space-y-6 p-6">
		<!-- Summary strip -->
		<div class="grid gap-4 md:grid-cols-3">
			<Card>
				<CardHeader class="pb-2">
					<CardDescription>Total injections</CardDescription>
					<CardTitle class="text-2xl">{totalInjections.toLocaleString()}</CardTitle>
				</CardHeader>
				<CardContent>
					<div class="text-xs text-muted-foreground">
						{rules.filter((r) => r.enabled).length} active /
						{rules.length} total rule(s)
					</div>
				</CardContent>
			</Card>
			<Card class="md:col-span-2">
				<CardHeader class="pb-2">
					<CardDescription>Recent injections (last 5 min)</CardDescription>
					<CardTitle class="text-2xl">{recent.length}</CardTitle>
				</CardHeader>
				<CardContent>
					<div class="flex h-12 items-end gap-[2px]">
						{#each spark as count, i (i)}
							{@const h = (count / sparkMax) * 100}
							<div
								class="flex-1 rounded-sm bg-orange-500/70"
								style="height: {h}%; min-height: {count > 0 ? '4px' : '0'};"
								title={count > 0 ? `${count} injection(s)` : ''}
							></div>
						{/each}
					</div>
				</CardContent>
			</Card>
		</div>

		<!-- Presets -->
		<section>
			<h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
				Presets
			</h2>
			{#if presets.length === 0}
				<EmptyState icon={FlameIcon} title="Loading presets…" description="" />
			{:else}
				<div class="grid gap-3 md:grid-cols-2 lg:grid-cols-3">
					{#each presets as preset (preset.name)}
						<Card>
							<CardHeader>
								<CardTitle class="text-base">{preset.name}</CardTitle>
								<CardDescription>{preset.description}</CardDescription>
							</CardHeader>
							<CardContent>
								<Button
									size="sm"
									variant="outline"
									onclick={() => applyPreset(preset.name)}
								>
									<FlameIcon class="mr-1 h-4 w-4" />
									Apply
								</Button>
							</CardContent>
						</Card>
					{/each}
				</div>
			{/if}
		</section>

		<!-- Rules table -->
		<section>
			<h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
				Active rules
			</h2>
			{#if rules.length === 0}
				<EmptyState
					icon={FlameIcon}
					title="No chaos rules"
					description="Apply a preset above or click 'Add rule' to start injecting failures."
				/>
			{:else}
				<div class="rounded border border-border">
					<Table>
						<TableHeader>
							<TableRow>
								<TableHead class="w-[80px]">On</TableHead>
								<TableHead>Service</TableHead>
								<TableHead>Operation</TableHead>
								<TableHead class="w-[80px]">P</TableHead>
								<TableHead>Effect</TableHead>
								<TableHead>Label</TableHead>
								<TableHead class="text-right">Fired</TableHead>
								<TableHead class="w-[50px]"></TableHead>
							</TableRow>
						</TableHeader>
						<TableBody>
							{#each rules as rule (rule.id)}
								<TableRow>
									<TableCell>
										<Switch
											checked={rule.enabled}
											onCheckedChange={(v) => toggleRule(rule.id, v)}
										/>
									</TableCell>
									<TableCell class="font-mono text-xs"
										>{formatService(rule.service)}</TableCell
									>
									<TableCell class="font-mono text-xs"
										>{formatOperation(rule.operation)}</TableCell
									>
									<TableCell class="font-mono text-xs"
										>{rule.probability.toFixed(2)}</TableCell
									>
									<TableCell><Badge>{formatEffect(rule.effect)}</Badge></TableCell>
									<TableCell class="text-xs text-muted-foreground">
										<div>{rule.label ?? '—'}</div>
										{#if describeSchedule(rule.schedule)}
											<div class="mt-0.5 text-[11px] text-orange-500/80">
												⏱ {describeSchedule(rule.schedule)}
											</div>
										{/if}
									</TableCell>
									<TableCell class="text-right font-mono text-xs"
										>{rule.injection_count}</TableCell
									>
									<TableCell>
										<Button
											size="sm"
											variant="ghost"
											onclick={() => deleteRule(rule.id)}
										>
											<Trash2Icon class="h-4 w-4" />
										</Button>
									</TableCell>
								</TableRow>
							{/each}
						</TableBody>
					</Table>
				</div>
			{/if}
		</section>

		<!-- Recent injections list -->
		{#if recent.length > 0}
			<section>
				<h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
					Recent injections
				</h2>
				<div class="rounded border border-border">
					<Table>
						<TableHeader>
							<TableRow>
								<TableHead class="w-[120px]">When</TableHead>
								<TableHead>Service</TableHead>
								<TableHead>Operation</TableHead>
								<TableHead>Rule</TableHead>
							</TableRow>
						</TableHeader>
						<TableBody>
							{#each [...recent].reverse().slice(0, 30) as inj, i (i)}
								<TableRow>
									<TableCell class="text-xs text-muted-foreground"
										>{formatTs(inj.ts)}</TableCell
									>
									<TableCell class="font-mono text-xs">{inj.service}</TableCell>
									<TableCell class="font-mono text-xs"
										>{inj.operation ?? '—'}</TableCell
									>
									<TableCell class="font-mono text-xs"
										>{inj.rule_id.slice(0, 8)}</TableCell
									>
								</TableRow>
							{/each}
						</TableBody>
					</Table>
				</div>
			</section>
		{/if}

		{#if lastUpdated}
			<div class="text-right text-xs text-muted-foreground">
				Updated {lastUpdated.toLocaleTimeString()}
			</div>
		{/if}
	</div>
</ServicePage>

<ConfirmDialog
	bind:open={clearOpen}
	title="Clear all chaos rules?"
	description="Delete every chaos rule and reset the injection counters. This cannot be undone."
	confirmLabel="Clear all"
	busy={clearBusy}
	onConfirm={confirmClearAll}
	onClose={() => (clearOpen = false)}
/>
