<script lang="ts">
	// Unified view of model mappings: aliases (multi-target with
	// strategy) + legacy single-target entries from [invoke]/[embed].
	// Edits always write to [aliases]; editing a legacy row promotes
	// it to an alias and drops it from the legacy table. The runtime
	// already prefers aliases over legacy entries, so promotion is
	// safe and reversible (delete the alias to revert).
	import { onMount, onDestroy } from 'svelte';
	import {
		getRuntimeConfig,
		putRuntimeConfig,
		type AliasKind,
		type AliasSpec,
		type BedrockSpec,
		type ModelMapEntry,
		type ModelPricing,
		type RuntimeConfig,
		type RuntimeConfigEnvelope,
	} from '$lib/api/runtime-config';
	import {
		getGatewayCatalog,
		getGatewayMetrics,
		testGatewayPrompt,
		type CatalogProvider,
		type MetricMappingRow,
		type ProviderCatalog,
		type TestPromptResult,
	} from '$lib/api/gateway';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import {
		Table,
		TableBody,
		TableCell,
		TableHead,
		TableHeader,
		TableRow,
	} from '$lib/components/ui/table';
	import { EmptyState } from '$lib/components/service';
	import AliasEditorDialog from './alias-editor-dialog.svelte';
	import { Textarea } from '$lib/components/ui/textarea';
	import BoxesIcon from '@lucide/svelte/icons/boxes';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import SearchIcon from '@lucide/svelte/icons/search';
	import PlayIcon from '@lucide/svelte/icons/play';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import CircleXIcon from '@lucide/svelte/icons/circle-x';
	import { toast } from 'svelte-sonner';

	type FilterKind = 'all' | 'chat' | 'embed';

	interface Row {
		id: string;
		kind: AliasKind;
		/** 'alias' = multi-target with strategy, 'legacy' = single-target invoke/embed entry. */
		source: 'alias' | 'legacy';
		targets: Array<{ backend: string; tag: string }>;
	}

	let envelope = $state<RuntimeConfigEnvelope | null>(null);
	let catalog = $state<ProviderCatalog | null>(null);
	let loading = $state(true);
	let savingAction = $state(false);
	let metricsByMapping = $state<Record<string, MetricMappingRow[]>>({});
	let metricsTimer: ReturnType<typeof setInterval> | null = null;

	let query = $state('');
	let kindFilter = $state<FilterKind>('all');

	let dialogOpen = $state(false);
	let dialogMode = $state<'add' | 'edit'>('add');
	let dialogInitial = $state<{
		id: string;
		alias: AliasSpec;
		pricing?: ModelPricing | null;
	} | null>(null);

	// Inline-tester UI state. Each chat row can expand to a small
	// prompt panel; only one tester is "running" at a time so we
	// keep a shared `running` flag rather than per-row.
	let testerOpen = $state<Record<string, boolean>>({});
	let testerPrompt = $state<Record<string, string>>({});
	let testerResult = $state<Record<string, TestPromptResult | null>>({});
	let testerRunning = $state<Record<string, boolean>>({});

	function rowKey(r: Row): string {
		return `${r.id}|${r.kind}`;
	}

	function toggleTester(r: Row) {
		const k = rowKey(r);
		testerOpen[k] = !testerOpen[k];
		if (testerOpen[k] && testerPrompt[k] === undefined) {
			testerPrompt[k] = 'Reply with a one-line confirmation.';
		}
	}

	async function runTester(r: Row) {
		const k = rowKey(r);
		const prompt = (testerPrompt[k] ?? '').trim();
		if (!prompt) {
			toast.error('Enter a prompt first.');
			return;
		}
		testerRunning[k] = true;
		try {
			testerResult[k] = await testGatewayPrompt(r.id, prompt);
			// Bump metrics so the Activity chips update without
			// waiting for the next 5s tick.
			void refreshMetrics();
		} catch (e) {
			testerResult[k] = {
				latencyMs: 0,
				response: null,
				error: e instanceof Error ? e.message : 'Test failed',
			};
		} finally {
			testerRunning[k] = false;
		}
	}

	onMount(async () => {
		try {
			const [env, cat] = await Promise.all([getRuntimeConfig(), getGatewayCatalog()]);
			envelope = env;
			catalog = cat;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load aliases');
		} finally {
			loading = false;
		}
		void refreshMetrics();
		metricsTimer = setInterval(refreshMetrics, 5000);
	});

	onDestroy(() => {
		if (metricsTimer !== null) clearInterval(metricsTimer);
	});

	// Per-bedrock-id rollup of the per-(id, backend) rows the
	// server returns. The Models table shows one row per bedrock
	// id, so we sum + percentile-pick across all backends to give
	// the headline chips.
	async function refreshMetrics() {
		try {
			const res = await getGatewayMetrics();
			const next: Record<string, MetricMappingRow[]> = {};
			for (const row of res.mappings) {
				(next[row.bedrockId] ??= []).push(row);
			}
			metricsByMapping = next;
		} catch {
			// Silent: metrics poll failure shouldn't spam toasts.
		}
	}

	function rollupForId(id: string): { total: number; p50: number | null; p95: number | null } {
		const rows = metricsByMapping[id] ?? [];
		if (rows.length === 0) return { total: 0, p50: null, p95: null };
		let total = 0;
		let p50 = 0;
		let p95 = 0;
		// Aggregate as the max of per-backend percentiles. Exact
		// percentile-of-the-union would need bucket merging server-
		// side; max-of-percentiles is the conservative read for a
		// glance, and matches what the UI badge implies ("slowest
		// you'd typically see").
		for (const r of rows) {
			total += r.total;
			if (r.p50Ms !== null) p50 = Math.max(p50, r.p50Ms);
			if (r.p95Ms !== null) p95 = Math.max(p95, r.p95Ms);
		}
		return { total, p50: p50 || null, p95: p95 || null };
	}

	async function reload() {
		loading = true;
		try {
			envelope = await getRuntimeConfig();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to reload');
		} finally {
			loading = false;
		}
	}

	let backendOptions = $derived.by<Array<{ name: string; providerKey: string | null }>>(() => {
		const map = envelope?.config.bedrock.spec.backends ?? {};
		return Object.entries(map)
			.map(([name, b]) => ({ name, providerKey: b.provider ?? null }))
			.sort((a, b) => a.name.localeCompare(b.name));
	});

	let defaultBackend = $derived(envelope?.config.bedrock.spec.default_backend ?? null);

	// Build a unified row list across aliases + legacy tables. Alias
	// entries shadow legacy ones with the same id (matches resolver
	// precedence), so we render exactly one row per id.
	let rows = $derived.by<Row[]>(() => {
		const spec = envelope?.config.bedrock.spec;
		if (!spec) return [];
		const out: Row[] = [];
		const aliases = spec.aliases ?? {};
		for (const [id, a] of Object.entries(aliases)) {
			out.push({
				id,
				kind: a.kind ?? 'chat',
				source: 'alias',
				targets: a.targets.map((t) => ({ backend: t.backend, tag: t.tag })),
			});
		}
		const shadowed = new Set(Object.keys(aliases));
		const legacyToTargets = (entry: ModelMapEntry): { backend: string; tag: string } => {
			if (typeof entry === 'string') {
				return { backend: defaultBackend ?? '(default)', tag: entry };
			}
			return { backend: entry.backend, tag: entry.tag };
		};
		for (const [id, e] of Object.entries(spec.invoke)) {
			if (shadowed.has(id)) continue;
			out.push({ id, kind: 'chat', source: 'legacy', targets: [legacyToTargets(e)] });
		}
		for (const [id, e] of Object.entries(spec.embed)) {
			if (shadowed.has(id)) continue;
			out.push({ id, kind: 'embed', source: 'legacy', targets: [legacyToTargets(e)] });
		}
		out.sort((a, b) => a.id.localeCompare(b.id));
		return out;
	});

	let filteredRows = $derived.by<Row[]>(() => {
		const q = query.trim().toLowerCase();
		return rows.filter((r) => {
			if (kindFilter !== 'all' && r.kind !== kindFilter) return false;
			if (!q) return true;
			if (r.id.toLowerCase().includes(q)) return true;
			if (r.targets.some((t) => t.backend.toLowerCase().includes(q) || t.tag.toLowerCase().includes(q))) {
				return true;
			}
			return false;
		});
	});

	let existingAliasIds = $derived.by<string[]>(() => {
		const spec = envelope?.config.bedrock.spec;
		return Object.keys(spec?.aliases ?? {});
	});

	function nextSpec(mutate: (spec: BedrockSpec) => void): RuntimeConfig | null {
		if (!envelope) return null;
		const cloned: BedrockSpec = JSON.parse(JSON.stringify(envelope.config.bedrock.spec));
		if (!cloned.aliases) cloned.aliases = {};
		mutate(cloned);
		return {
			...envelope.config,
			bedrock: { ...envelope.config.bedrock, spec: cloned },
		};
	}

	async function applyMutation(
		mutate: (spec: BedrockSpec) => void,
		successMsg: string,
	): Promise<boolean> {
		const next = nextSpec(mutate);
		if (!next) return false;
		savingAction = true;
		try {
			envelope = await putRuntimeConfig(next);
			toast.success(successMsg);
			return true;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Save failed');
			return false;
		} finally {
			savingAction = false;
		}
	}

	function openAdd() {
		dialogMode = 'add';
		dialogInitial = null;
		dialogOpen = true;
	}

	function pricingFor(id: string): ModelPricing | null {
		return envelope?.config.bedrock.spec.pricing?.[id] ?? null;
	}

	function openEdit(row: Row) {
		dialogMode = 'edit';
		// Promote legacy entry to an alias-shaped initial value so
		// the editor speaks one language. Save handler will drop the
		// legacy entry if it existed.
		dialogInitial = {
			id: row.id,
			alias: {
				kind: row.kind,
				strategy: 'first',
				targets: row.targets.map((t) => ({ backend: t.backend, tag: t.tag })),
			},
			pricing: pricingFor(row.id),
		};
		dialogOpen = true;
	}

	async function handleSubmit(result: {
		id: string;
		alias: AliasSpec;
		pricing?: ModelPricing | null;
	}) {
		const id = result.id;
		const wasLegacy = !(envelope?.config.bedrock.spec.aliases ?? {})[id];
		await applyMutation((spec) => {
			(spec.aliases ??= {})[id] = result.alias;
			// If we just promoted a legacy entry, drop it from the
			// legacy table that matches the alias kind. Leaves the
			// other table alone (alias resolver only consults the
			// matching kind, so a stray entry won't be exercised).
			if (wasLegacy) {
				const table = result.alias.kind === 'embed' ? spec.embed : spec.invoke;
				delete table[id];
			}
			// Pricing: write through when set, delete when cleared so
			// the table doesn't bloat with empty entries.
			spec.pricing ??= {};
			if (result.pricing) {
				spec.pricing[id] = result.pricing;
			} else {
				delete spec.pricing[id];
			}
		}, wasLegacy && dialogMode === 'edit' ? 'Promoted to alias' : dialogMode === 'edit' ? 'Alias updated' : 'Mapping added');
	}

	async function remove(row: Row) {
		const msg = row.source === 'alias'
			? `Remove alias for '${row.id}'?`
			: `Remove legacy mapping for '${row.id}'?`;
		if (!confirm(msg)) return;
		await applyMutation((spec) => {
			if (row.source === 'alias') {
				delete (spec.aliases ??= {})[row.id];
			} else if (row.kind === 'chat') {
				delete spec.invoke[row.id];
			} else {
				delete spec.embed[row.id];
			}
			// Pricing belongs to the id, not the mapping kind, so it
			// goes away with the row.
			if (spec.pricing) delete spec.pricing[row.id];
		}, 'Mapping removed');
	}

	function providerNameForBackend(name: string): string | null {
		if (!catalog) return null;
		const opt = backendOptions.find((b) => b.name === name);
		if (!opt?.providerKey) return null;
		const p = catalog.providers.find((p: CatalogProvider) => p.key === opt.providerKey);
		return p?.name ?? null;
	}
</script>

<div class="space-y-4 p-4">
	<section class="rounded-lg border bg-card p-4 text-sm">
		<div class="flex items-start gap-3">
			<BoxesIcon class="mt-0.5 h-5 w-5 shrink-0 text-muted-foreground" />
			<div class="space-y-1">
				<p class="font-semibold">Models &amp; Aliases</p>
				<p class="text-muted-foreground">
					Map a Bedrock model id to an ordered list of backend targets. The first target whose
					backend is healthy wins at runtime; if it returns a retriable error (5xx / 408 / 429 /
					timeout) the gateway rolls forward to the next target automatically. Per-target
					overrides for timeout, max tokens, and temperature live on the editor.
				</p>
			</div>
		</div>
	</section>

	<header class="flex items-center justify-between">
		<div>
			<h2 class="text-lg font-semibold">Mappings</h2>
			<p class="text-sm text-muted-foreground">
				{rows.length} total
				{#if rows.length > 0}
					· {rows.filter((r) => r.kind === 'chat').length} chat
					· {rows.filter((r) => r.kind === 'embed').length} embed
					{@const withFallback = rows.filter((r) => r.targets.length > 1).length}
					{#if withFallback > 0}
						· {withFallback} with fallback
					{/if}
				{/if}
			</p>
		</div>
		<div class="flex gap-2">
			<Button variant="ghost" size="sm" onclick={reload} disabled={loading || savingAction}>
				<RefreshCwIcon class={loading ? 'h-4 w-4 animate-spin' : 'h-4 w-4'} />
				<span class="ml-2">Reload</span>
			</Button>
			<Button size="sm" onclick={openAdd} disabled={!envelope || backendOptions.length === 0}>
				<PlusIcon class="h-4 w-4" />
				<span class="ml-2">Add mapping</span>
			</Button>
		</div>
	</header>

	<div class="flex flex-wrap items-center gap-2">
		<div class="relative max-w-xs flex-1">
			<SearchIcon
				class="pointer-events-none absolute left-2 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
			/>
			<Input bind:value={query} placeholder="Search id or target…" class="pl-8" />
		</div>
		{#each ['all', 'chat', 'embed'] as const as k (k)}
			<Button
				variant={kindFilter === k ? 'default' : 'outline'}
				size="sm"
				onclick={() => (kindFilter = k)}
			>
				{k}
			</Button>
		{/each}
	</div>

	{#if loading && rows.length === 0}
		<EmptyState icon={BoxesIcon} title="Loading mappings…" />
	{:else if backendOptions.length === 0}
		<EmptyState
			icon={BoxesIcon}
			title="No backends configured"
			description="Add a backend in the Backends tab first — aliases need somewhere to route to."
		/>
	{:else if filteredRows.length === 0}
		<EmptyState
			icon={BoxesIcon}
			title={rows.length === 0 ? 'No mappings yet' : 'No mappings match the filter'}
			description={rows.length === 0
				? "Bedrock InvokeModel calls fall through to the built-in defaults (Anthropic/Llama/Mistral/etc. map to llama3.1:8b). Add a mapping to override or to wire a multi-target alias."
				: 'Try a different search or filter.'}
		/>
	{:else}
		<div class="rounded-lg border bg-card">
			<Table>
				<TableHeader>
					<TableRow>
						<TableHead>Bedrock id</TableHead>
						<TableHead>Kind</TableHead>
						<TableHead>Targets</TableHead>
						<TableHead>Activity</TableHead>
						<TableHead class="text-right">Actions</TableHead>
					</TableRow>
				</TableHeader>
				<TableBody>
					{#each filteredRows as row (row.id + '|' + row.kind)}
						{@const stats = rollupForId(row.id)}
						{@const priced = pricingFor(row.id)}
						<TableRow>
							<TableCell class="font-mono text-xs">{row.id}</TableCell>
							<TableCell>
								<Badge variant={row.kind === 'embed' ? 'outline' : 'secondary'} class="text-[10px] uppercase">
									{row.kind}
								</Badge>
								{#if row.targets.length > 1}
									<Badge variant="default" class="ml-1 text-[10px] uppercase" title="Has fallback targets">
										+{row.targets.length - 1}
									</Badge>
								{/if}
								{#if priced}
									<Badge
										variant="outline"
										class="ml-1 font-mono text-[10px]"
										title={`Pricing override: in $${priced.input_per_million_tokens ?? 0}/MTok · out $${priced.output_per_million_tokens ?? 0}/MTok`}
									>
										${priced.input_per_million_tokens ?? 0}/${priced.output_per_million_tokens ?? 0}
									</Badge>
								{/if}
							</TableCell>
							<TableCell>
								<div class="flex flex-col gap-1">
									{#each row.targets as t, i (i + '|' + t.backend + '|' + t.tag)}
										{@const provName = providerNameForBackend(t.backend)}
										<div class="flex items-center gap-2 text-xs">
											{#if row.targets.length > 1}
												<span class="rounded bg-muted/40 px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
													#{i + 1}
												</span>
											{/if}
											<code class="font-mono">{t.backend}</code>
											{#if provName}
												<Badge variant="outline" class="text-[10px]">{provName}</Badge>
											{/if}
											<span class="text-muted-foreground">→</span>
											<code class="font-mono">{t.tag}</code>
										</div>
									{/each}
								</div>
							</TableCell>
							<TableCell>
								{#if stats.total === 0}
									<span class="text-xs text-muted-foreground">—</span>
								{:else}
									<div class="flex flex-wrap items-center gap-1 text-[10px]">
										<Badge variant="outline" class="font-mono">
											{stats.total} call{stats.total === 1 ? '' : 's'}
										</Badge>
										{#if stats.p50 !== null}
											<Badge variant="secondary" class="font-mono">
												p50 {stats.p50}ms
											</Badge>
										{/if}
										{#if stats.p95 !== null}
											<Badge variant="secondary" class="font-mono">
												p95 {stats.p95}ms
											</Badge>
										{/if}
									</div>
								{/if}
							</TableCell>
							<TableCell class="text-right">
								<div class="flex justify-end gap-1">
									{#if row.kind === 'chat'}
										{@const tk = rowKey(row)}
										<Button
											variant="ghost"
											size="icon"
											onclick={() => toggleTester(row)}
											disabled={savingAction}
											aria-label="Test"
											title="Send test prompt"
										>
											<PlayIcon
												class={testerOpen[tk]
													? 'h-4 w-4 rotate-90 transition-transform'
													: 'h-4 w-4 transition-transform'}
											/>
										</Button>
									{/if}
									<Button
										variant="ghost"
										size="icon"
										onclick={() => openEdit(row)}
										disabled={savingAction}
										aria-label="Edit"
									>
										<PencilIcon class="h-4 w-4" />
									</Button>
									<Button
										variant="ghost"
										size="icon"
										onclick={() => remove(row)}
										disabled={savingAction}
										aria-label="Remove"
									>
										<Trash2Icon class="h-4 w-4" />
									</Button>
								</div>
							</TableCell>
						</TableRow>
						{#if row.kind === 'chat'}
							{@const tk = rowKey(row)}
							{#if testerOpen[tk]}
								{@const res = testerResult[tk]}
								{@const running = testerRunning[tk] ?? false}
								<TableRow>
									<TableCell colspan={5} class="bg-muted/30">
										<div class="flex flex-col gap-2 py-2">
											<div class="flex items-start gap-2">
												<ChevronRightIcon class="mt-2 h-4 w-4 shrink-0 text-muted-foreground" />
												<div class="flex-1">
													<Textarea
														rows={2}
														placeholder="Reply with a one-line confirmation."
														value={testerPrompt[tk] ?? ''}
														oninput={(e) => (testerPrompt[tk] = e.currentTarget.value)}
													/>
												</div>
												<Button
													size="sm"
													onclick={() => runTester(row)}
													disabled={running}
												>
													<PlayIcon class={running ? 'h-4 w-4 animate-pulse' : 'h-4 w-4'} />
													<span class="ml-1">{running ? 'Running…' : 'Run'}</span>
												</Button>
											</div>
											{#if res}
												<div class="rounded border bg-background p-2 text-xs">
													<div class="mb-1 flex flex-wrap items-center gap-2 text-muted-foreground">
														{#if res.error}
															<CircleXIcon class="h-3.5 w-3.5 text-rose-500" />
															<span class="font-semibold text-rose-600">Failed</span>
														{:else}
															<CircleCheckIcon class="h-3.5 w-3.5 text-emerald-500" />
															<span class="font-semibold text-emerald-600">OK</span>
														{/if}
														<span class="font-mono">{res.latencyMs}ms</span>
														{#if res.usage}
															{#if res.usage.inputTokens !== null}
																<Badge variant="outline" class="font-mono text-[10px]">
																	in {res.usage.inputTokens}t
																</Badge>
															{/if}
															{#if res.usage.outputTokens !== null}
																<Badge variant="outline" class="font-mono text-[10px]">
																	out {res.usage.outputTokens}t
																</Badge>
															{/if}
															{#if res.usage.totalCost !== null && res.usage.totalCost > 0}
																<Badge variant="secondary" class="font-mono text-[10px]" title={`input $${(res.usage.inputCost ?? 0).toFixed(6)} + output $${(res.usage.outputCost ?? 0).toFixed(6)}`}>
																	${res.usage.totalCost.toFixed(6)}
																</Badge>
															{/if}
														{/if}
													</div>
													{#if res.error}
														<pre class="whitespace-pre-wrap text-rose-600">{res.error}</pre>
													{:else if res.response}
														<pre class="whitespace-pre-wrap">{res.response}</pre>
													{:else}
														<span class="text-muted-foreground">(empty response)</span>
													{/if}
												</div>
											{/if}
										</div>
									</TableCell>
								</TableRow>
							{/if}
						{/if}
					{/each}
				</TableBody>
			</Table>
		</div>
	{/if}
</div>

<AliasEditorDialog
	open={dialogOpen}
	mode={dialogMode}
	backends={backendOptions}
	{catalog}
	existingIds={existingAliasIds}
	initial={dialogInitial}
	onOpenChange={(o) => (dialogOpen = o)}
	onSubmit={handleSubmit}
/>
