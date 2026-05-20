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
		type RuntimeConfig,
		type RuntimeConfigEnvelope,
	} from '$lib/api/runtime-config';
	import {
		getGatewayCatalog,
		getGatewayMetrics,
		type CatalogProvider,
		type MetricMappingRow,
		type ProviderCatalog,
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
	import BoxesIcon from '@lucide/svelte/icons/boxes';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import SearchIcon from '@lucide/svelte/icons/search';
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
	let dialogInitial = $state<{ id: string; alias: AliasSpec } | null>(null);

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
		};
		dialogOpen = true;
	}

	async function handleSubmit(result: { id: string; alias: AliasSpec }) {
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
					backend exists wins at runtime; remaining targets cover the case where a backend was
					removed without cleaning up the mapping. Automatic fallback on errors/timeouts and
					per-target overrides arrive in Phases 4-5.
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
					· {rows.filter((r) => r.source === 'alias').length} alias{rows.filter((r) => r.source === 'alias').length === 1 ? '' : 'es'}
					· {rows.filter((r) => r.source === 'legacy').length} legacy
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
						<TableHead>Source</TableHead>
						<TableHead>Targets</TableHead>
						<TableHead>Activity</TableHead>
						<TableHead class="text-right">Actions</TableHead>
					</TableRow>
				</TableHeader>
				<TableBody>
					{#each filteredRows as row (row.id + '|' + row.kind)}
						{@const stats = rollupForId(row.id)}
						<TableRow>
							<TableCell class="font-mono text-xs">{row.id}</TableCell>
							<TableCell>
								<Badge variant={row.kind === 'embed' ? 'outline' : 'secondary'} class="text-[10px] uppercase">
									{row.kind}
								</Badge>
							</TableCell>
							<TableCell>
								{#if row.source === 'alias'}
									<Badge variant="default" class="text-[10px] uppercase">alias</Badge>
								{:else}
									<Badge variant="outline" class="text-[10px] uppercase">legacy</Badge>
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
