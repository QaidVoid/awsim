<script lang="ts">
	import { onMount } from 'svelte';
	import {
		getRuntimeConfig,
		getRuntimeConfigDefaults,
		putRuntimeConfig,
		type BedrockBackendSpec,
		type ModelMapEntry,
		type RuntimeConfig,
		type RuntimeConfigEnvelope,
	} from '$lib/api/runtime-config';
	import {
		getBedrockDefaults,
		type BedrockDefaultsResponse,
		type BedrockModelMapEntry,
	} from '$lib/api/bedrock';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Switch } from '$lib/components/ui/switch';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { Badge } from '$lib/components/ui/badge';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import SaveIcon from '@lucide/svelte/icons/save';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import HardDriveIcon from '@lucide/svelte/icons/hard-drive';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import NetworkIcon from '@lucide/svelte/icons/network';
	import { route } from '$lib/url';
	import { toast } from 'svelte-sonner';

	// UI-shape rows. We expand the wire `Record<string, …>` into arrays
	// for ergonomic add/remove; serialise back on save. Invoke/embed
	// use a plain `tag + optional backend` shape so the user doesn't
	// have to remember the wire-level untagged enum.
	interface BackendRow {
		name: string;
		endpoint: string;
		apiKey: string;
		apiKeyEnv: string;
		keyMode: 'none' | 'inline' | 'env';
	}
	interface MapRow {
		id: string;
		tag: string;
		backend: string;
	}

	let envelope = $state<RuntimeConfigEnvelope | null>(null);
	let defaults = $state<BedrockDefaultsResponse | null>(null);
	let configDefaults = $state<RuntimeConfig | null>(null);
	let loading = $state(true);
	let saving = $state(false);

	let bedrockEnabled = $state(false);
	let defaultBackend = $state('');
	let backendRows = $state<BackendRow[]>([]);
	let invokeRows = $state<MapRow[]>([]);
	let embedRows = $state<MapRow[]>([]);
	let sesRetentionHours = $state(720);
	let iamEnforce = $state(false);
	let logLevel = $state('info');

	onMount(load);

	async function load() {
		loading = true;
		try {
			const [env, defs, cfgDefs] = await Promise.all([
				getRuntimeConfig(),
				getBedrockDefaults(),
				getRuntimeConfigDefaults(),
			]);
			envelope = env;
			defaults = defs;
			configDefaults = cfgDefs;
			seedFromEnvelope(env.config);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load runtime config');
		} finally {
			loading = false;
		}
	}

	function resetBedrock() {
		if (!configDefaults) return;
		bedrockEnabled = configDefaults.bedrock.enabled;
		defaultBackend = configDefaults.bedrock.spec.default_backend ?? '';
		backendRows = Object.entries(configDefaults.bedrock.spec.backends).map(([name, b]) => {
			const keyMode: BackendRow['keyMode'] = b.api_key_env
				? 'env'
				: b.api_key
					? 'inline'
					: 'none';
			return {
				name,
				endpoint: b.endpoint,
				apiKey: b.api_key ?? '',
				apiKeyEnv: b.api_key_env ?? '',
				keyMode,
			};
		});
		invokeRows = entriesToRows(configDefaults.bedrock.spec.invoke);
		embedRows = entriesToRows(configDefaults.bedrock.spec.embed);
		toast.info('Bedrock section reset — Save to apply');
	}
	function resetSes() {
		if (!configDefaults) return;
		sesRetentionHours = configDefaults.ses.retention_hours;
		toast.info('SES section reset — Save to apply');
	}
	function resetIam() {
		if (!configDefaults) return;
		iamEnforce = configDefaults.iam.enforce;
		toast.info('IAM section reset — Save to apply');
	}
	function resetLogging() {
		if (!configDefaults) return;
		logLevel = configDefaults.logging.level;
		toast.info('Logging section reset — Save to apply');
	}

	// Derived "modified" flags per section. Compares current form
	// state to the server-side defaults snapshot. We compare the
	// shape the form will serialise (built via buildPayload) so this
	// stays consistent with what actually gets PUT.
	let isBedrockModified = $derived.by(() => {
		if (!configDefaults) return false;
		const current = buildPayload().bedrock;
		const def = configDefaults.bedrock;
		return JSON.stringify(current) !== JSON.stringify(def);
	});
	let isSesModified = $derived(
		!!configDefaults && sesRetentionHours !== configDefaults.ses.retention_hours
	);
	let isIamModified = $derived(!!configDefaults && iamEnforce !== configDefaults.iam.enforce);
	let isLoggingModified = $derived(
		!!configDefaults && (logLevel.trim() || 'info') !== configDefaults.logging.level
	);

	function applyOllamaPreset() {
		bedrockEnabled = true;
		defaultBackend = 'ollama';
		backendRows = [
			{
				name: 'ollama',
				endpoint: 'http://localhost:11434/v1',
				apiKey: '',
				apiKeyEnv: '',
				keyMode: 'none',
			},
		];
		// Leave invoke/embed empty so the built-in defaults govern.
		invokeRows = [];
		embedRows = [];
		toast.info('Ollama preset filled in — Save to apply');
	}

	// Set of Bedrock ids that the user has overridden, for shadow-
	// highlighting the built-in defaults table.
	let invokeOverrideIds = $derived(
		new Set(invokeRows.map((r) => r.id.trim()).filter((s) => s.length > 0))
	);
	let embedOverrideIds = $derived(
		new Set(embedRows.map((r) => r.id.trim()).filter((s) => s.length > 0))
	);

	function seedFromEnvelope(cfg: RuntimeConfig) {
		bedrockEnabled = cfg.bedrock.enabled;
		defaultBackend = cfg.bedrock.spec.default_backend ?? '';
		backendRows = Object.entries(cfg.bedrock.spec.backends).map(([name, b]) => {
			const keyMode: BackendRow['keyMode'] = b.api_key_env
				? 'env'
				: b.api_key
					? 'inline'
					: 'none';
			return {
				name,
				endpoint: b.endpoint,
				apiKey: b.api_key ?? '',
				apiKeyEnv: b.api_key_env ?? '',
				keyMode,
			};
		});
		invokeRows = entriesToRows(cfg.bedrock.spec.invoke);
		embedRows = entriesToRows(cfg.bedrock.spec.embed);
		sesRetentionHours = cfg.ses.retention_hours;
		iamEnforce = cfg.iam.enforce;
		logLevel = cfg.logging.level;
	}

	function entriesToRows(record: Record<string, ModelMapEntry>): MapRow[] {
		return Object.entries(record).map(([id, entry]) => {
			if (typeof entry === 'string') return { id, tag: entry, backend: '' };
			return { id, tag: entry.tag, backend: entry.backend };
		});
	}

	function rowsToEntries(rows: MapRow[]): Record<string, ModelMapEntry> {
		const out: Record<string, ModelMapEntry> = {};
		for (const r of rows) {
			if (!r.id.trim() || !r.tag.trim()) continue;
			if (r.backend.trim()) {
				out[r.id.trim()] = { backend: r.backend.trim(), tag: r.tag.trim() };
			} else {
				out[r.id.trim()] = r.tag.trim();
			}
		}
		return out;
	}

	function buildPayload(): RuntimeConfig {
		const backends: Record<string, BedrockBackendSpec> = {};
		for (const r of backendRows) {
			if (!r.name.trim() || !r.endpoint.trim()) continue;
			const spec: BedrockBackendSpec = { endpoint: r.endpoint.trim() };
			if (r.keyMode === 'inline' && r.apiKey.trim()) spec.api_key = r.apiKey.trim();
			if (r.keyMode === 'env' && r.apiKeyEnv.trim()) spec.api_key_env = r.apiKeyEnv.trim();
			backends[r.name.trim()] = spec;
		}
		return {
			bedrock: {
				enabled: bedrockEnabled,
				spec: {
					default_backend: defaultBackend.trim() || null,
					backends,
					invoke: rowsToEntries(invokeRows),
					embed: rowsToEntries(embedRows),
				},
			},
			ses: { retention_hours: sesRetentionHours },
			iam: { enforce: iamEnforce },
			logging: { level: logLevel.trim() || 'info' },
		};
	}

	async function save() {
		saving = true;
		try {
			const payload = buildPayload();
			envelope = await putRuntimeConfig(payload);
			seedFromEnvelope(envelope.config);
			toast.success('Settings saved');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Save failed');
		} finally {
			saving = false;
		}
	}

	function addBackend() {
		backendRows = [
			...backendRows,
			{ name: '', endpoint: 'http://localhost:11434/v1', apiKey: '', apiKeyEnv: '', keyMode: 'none' },
		];
	}
	function removeBackend(i: number) {
		const removedName = backendRows[i].name;
		backendRows = backendRows.filter((_, idx) => idx !== i);
		// Cascade: anything that referenced this backend by name now
		// has a dangling pointer. Wipe to empty (= "use default" for
		// mappings, "no default" for the default-backend field).
		if (removedName) cascadeBackendChange(removedName, '');
	}
	function onBackendNameChange(i: number, newName: string) {
		const oldName = backendRows[i].name;
		backendRows[i].name = newName;
		if (oldName === newName || !oldName) return;
		// Renames propagate to every field referencing the old name so
		// the UI doesn't end up with stale references that vanish from
		// the dropdown options.
		cascadeBackendChange(oldName, newName);
	}
	function cascadeBackendChange(from: string, to: string) {
		if (defaultBackend === from) defaultBackend = to;
		for (let j = 0; j < invokeRows.length; j++) {
			if (invokeRows[j].backend === from) invokeRows[j].backend = to;
		}
		for (let j = 0; j < embedRows.length; j++) {
			if (embedRows[j].backend === from) embedRows[j].backend = to;
		}
	}
	function addInvoke() {
		invokeRows = [...invokeRows, { id: '', tag: '', backend: '' }];
	}
	function removeInvoke(i: number) {
		invokeRows = invokeRows.filter((_, idx) => idx !== i);
	}
	function addEmbed() {
		embedRows = [...embedRows, { id: '', tag: '', backend: '' }];
	}
	function removeEmbed(i: number) {
		embedRows = embedRows.filter((_, idx) => idx !== i);
	}

	let backendNames = $derived(
		backendRows.map((r) => r.name.trim()).filter((n) => n.length > 0)
	);
</script>

{#snippet headerActions()}
	<Button variant="ghost" size="sm" onclick={load} disabled={loading || saving}>
		<RefreshCwIcon class={loading ? 'h-4 w-4 animate-spin' : 'h-4 w-4'} />
		<span class="ml-2">Reload</span>
	</Button>
	<Button size="sm" onclick={save} disabled={loading || saving}>
		<SaveIcon class="h-4 w-4" />
		<span class="ml-2">{saving ? 'Saving…' : 'Save'}</span>
	</Button>
{/snippet}

<ServicePage
	title="Settings"
	description="Hot-reloadable runtime configuration. Changes apply immediately."
	actions={headerActions}
>
	<div class="space-y-6 p-6">
		{#if envelope && !envelope.persistent}
			<Alert>
				<CircleAlertIcon class="h-4 w-4" />
				<AlertTitle>In-memory only</AlertTitle>
				<AlertDescription>
					Changes apply for the current run but reset on restart. Pass <code>--data-dir</code> to persist settings.
				</AlertDescription>
			</Alert>
		{:else if envelope?.persistent && envelope.configPath}
			<Alert>
				<HardDriveIcon class="h-4 w-4" />
				<AlertTitle>Persisted</AlertTitle>
				<AlertDescription>
					Settings save to <code class="font-mono text-xs">{envelope.configPath}</code> and survive restarts.
				</AlertDescription>
			</Alert>
		{/if}

		<!-- Bedrock section -->
		<section class="rounded-lg border bg-card">
			<header class="flex items-start justify-between gap-4 border-b p-4">
				<div>
					<div class="flex items-center gap-2">
						<h2 class="text-base font-semibold">Bedrock proxy</h2>
						{#if isBedrockModified}
							<Badge variant="secondary" class="text-[10px]">modified</Badge>
						{/if}
					</div>
					<p class="mt-1 text-sm text-muted-foreground">
						OpenAI-compatible backends serving Bedrock InvokeModel / Converse / embeddings.
						Disable to fall back to canned responses.
					</p>
				</div>
				<div class="flex items-center gap-2 pt-1">
					{#if isBedrockModified}
						<Button variant="ghost" size="sm" onclick={resetBedrock}>
							<RotateCcwIcon class="h-4 w-4" />
							<span class="ml-1">Reset</span>
						</Button>
					{/if}
					<Label for="bedrock-enabled" class="text-sm">Enabled</Label>
					<Switch id="bedrock-enabled" bind:checked={bedrockEnabled} />
				</div>
			</header>

			<div class="space-y-6 p-4">
				<Alert>
					<NetworkIcon class="h-4 w-4" />
					<AlertTitle>Moving to Model Gateway</AlertTitle>
					<AlertDescription>
						These controls are migrating to the new
						<a class="underline" href={route('/gateway')}>Model Gateway</a> page,
						which gains a provider catalog, reusable credentials, alias groups
						with automatic fallback, background health pings, and per-mapping
						overrides. Edits here still work for now; this section will be
						removed once the new UI reaches parity.
					</AlertDescription>
				</Alert>

				<div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
					<div>
						<Label for="default-backend">Default backend</Label>
						<Select
							type="single"
							bind:value={defaultBackend}
							disabled={backendNames.length === 0}
						>
							<SelectTrigger id="default-backend" class="w-full">
								{defaultBackend ? defaultBackend : '(none)'}
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="" label="(none)">(none)</SelectItem>
								{#each backendNames as n (n)}
									<SelectItem value={n} label={n}>{n}</SelectItem>
								{/each}
							</SelectContent>
						</Select>
						<p class="mt-1 text-xs text-muted-foreground">
							Bare-tag mappings route here when no backend is pinned on the entry.
						</p>
					</div>
				</div>

				<div>
					<div class="mb-2 flex items-center justify-between">
						<h3 class="text-sm font-semibold">Backends</h3>
						<div class="flex gap-2">
							<Button variant="outline" size="sm" onclick={applyOllamaPreset}>
								<SparklesIcon class="h-4 w-4" />
								<span class="ml-1">Use Ollama preset</span>
							</Button>
							<Button variant="outline" size="sm" onclick={addBackend}>
								<PlusIcon class="h-4 w-4" />
								<span class="ml-1">Add</span>
							</Button>
						</div>
					</div>
					<div class="space-y-2">
						{#each backendRows as row, i (i)}
							<div class="grid grid-cols-12 items-end gap-2 rounded border p-3">
								<div class="col-span-12 sm:col-span-3">
									<Label class="text-xs">Name</Label>
									<Input
									value={row.name}
									oninput={(e) => onBackendNameChange(i, e.currentTarget.value)}
									placeholder="ollama"
								/>
								</div>
								<div class="col-span-12 sm:col-span-5">
									<Label class="text-xs">Endpoint</Label>
									<Input bind:value={row.endpoint} placeholder="http://host:port/v1" />
								</div>
								<div class="col-span-6 sm:col-span-2">
									<Label class="text-xs">Auth</Label>
									<Select
										type="single"
										value={row.keyMode}
										onValueChange={(v) => (row.keyMode = v as BackendRow['keyMode'])}
									>
										<SelectTrigger class="w-full">
											{row.keyMode === 'inline'
												? 'Inline key'
												: row.keyMode === 'env'
													? 'Env var'
													: 'None'}
										</SelectTrigger>
										<SelectContent>
											<SelectItem value="none" label="None">None</SelectItem>
											<SelectItem value="inline" label="Inline key">Inline key</SelectItem>
											<SelectItem value="env" label="Env var">Env var</SelectItem>
										</SelectContent>
									</Select>
								</div>
								<div class="col-span-12 sm:col-span-2 flex justify-end">
									<Button variant="ghost" size="icon" onclick={() => removeBackend(i)} aria-label="Remove">
										<Trash2Icon class="h-4 w-4" />
									</Button>
								</div>
								{#if row.keyMode === 'inline'}
									<div class="col-span-12">
										<Label class="text-xs">API key</Label>
										<Input
											type="password"
											bind:value={row.apiKey}
											placeholder="sk-..."
										/>
										<p class="mt-1 text-xs text-muted-foreground">
											Stored in plain text — prefer env vars for shared machines.
										</p>
									</div>
								{:else if row.keyMode === 'env'}
									<div class="col-span-12">
										<Label class="text-xs">Env var name</Label>
										<Input bind:value={row.apiKeyEnv} placeholder="GROQ_API_KEY" />
										<p class="mt-1 text-xs text-muted-foreground">
											Resolved at apply time. Save fails if the variable is unset.
										</p>
									</div>
								{/if}
							</div>
						{:else}
							<p class="text-sm text-muted-foreground">
								No backends configured. Add one to route Bedrock invocations to a real LLM server.
							</p>
						{/each}
					</div>
				</div>

				<div>
					<div class="mb-2 flex items-center justify-between">
						<h3 class="text-sm font-semibold">Invoke / Converse mappings</h3>
						<Button variant="outline" size="sm" onclick={addInvoke}>
							<PlusIcon class="h-4 w-4" />
							<span class="ml-1">Add</span>
						</Button>
					</div>
					<p class="mb-2 text-xs text-muted-foreground">
						Override the built-in defaults. Leave Backend empty to route through the default backend.
					</p>
					<div class="space-y-2">
						{#each invokeRows as row, i (i)}
							<div class="grid grid-cols-12 items-end gap-2 rounded border p-3">
								<div class="col-span-12 sm:col-span-5">
									<Label class="text-xs">Bedrock id</Label>
									<Input bind:value={row.id} placeholder="anthropic.claude-3-5-sonnet-..." />
								</div>
								<div class="col-span-6 sm:col-span-3">
									<Label class="text-xs">Backend</Label>
									<Select type="single" bind:value={row.backend}>
										<SelectTrigger class="w-full">
											{row.backend ? row.backend : '(default)'}
										</SelectTrigger>
										<SelectContent>
											<SelectItem value="" label="(default)">(default)</SelectItem>
											{#each backendNames as n (n)}
												<SelectItem value={n} label={n}>{n}</SelectItem>
											{/each}
										</SelectContent>
									</Select>
								</div>
								<div class="col-span-6 sm:col-span-3">
									<Label class="text-xs">Tag</Label>
									<Input bind:value={row.tag} placeholder="llama3.1:8b" />
								</div>
								<div class="col-span-12 sm:col-span-1 flex justify-end">
									<Button variant="ghost" size="icon" onclick={() => removeInvoke(i)} aria-label="Remove">
										<Trash2Icon class="h-4 w-4" />
									</Button>
								</div>
							</div>
						{:else}
							<p class="text-sm text-muted-foreground">
								Using built-in defaults only. Add an entry to override a specific Bedrock model id.
							</p>
						{/each}
					</div>
				</div>

				<div>
					<div class="mb-2 flex items-center justify-between">
						<h3 class="text-sm font-semibold">Embedding mappings</h3>
						<Button variant="outline" size="sm" onclick={addEmbed}>
							<PlusIcon class="h-4 w-4" />
							<span class="ml-1">Add</span>
						</Button>
					</div>
					<div class="space-y-2">
						{#each embedRows as row, i (i)}
							<div class="grid grid-cols-12 items-end gap-2 rounded border p-3">
								<div class="col-span-12 sm:col-span-5">
									<Label class="text-xs">Bedrock id</Label>
									<Input bind:value={row.id} placeholder="amazon.titan-embed-text-v2:0" />
								</div>
								<div class="col-span-6 sm:col-span-3">
									<Label class="text-xs">Backend</Label>
									<Select type="single" bind:value={row.backend}>
										<SelectTrigger class="w-full">
											{row.backend ? row.backend : '(default)'}
										</SelectTrigger>
										<SelectContent>
											<SelectItem value="" label="(default)">(default)</SelectItem>
											{#each backendNames as n (n)}
												<SelectItem value={n} label={n}>{n}</SelectItem>
											{/each}
										</SelectContent>
									</Select>
								</div>
								<div class="col-span-6 sm:col-span-3">
									<Label class="text-xs">Tag</Label>
									<Input bind:value={row.tag} placeholder="nomic-embed-text" />
								</div>
								<div class="col-span-12 sm:col-span-1 flex justify-end">
									<Button variant="ghost" size="icon" onclick={() => removeEmbed(i)} aria-label="Remove">
										<Trash2Icon class="h-4 w-4" />
									</Button>
								</div>
							</div>
						{:else}
							<p class="text-sm text-muted-foreground">
								Using built-in defaults only.
							</p>
						{/each}
					</div>
				</div>
			</div>
			{#if defaults}
				<div class="border-t bg-muted/20">
					<details class="group">
						<summary class="flex cursor-pointer items-center justify-between p-4 text-sm">
							<span>
								<span class="font-semibold">Built-in defaults</span>
								<span class="ml-2 text-xs text-muted-foreground">
									(invoke: {defaults.invoke.length} · embed: {defaults.embed.length})
								</span>
							</span>
							<span class="text-xs text-muted-foreground group-open:hidden">show</span>
							<span class="hidden text-xs text-muted-foreground group-open:inline">hide</span>
						</summary>
						<div class="px-4 pb-4 text-xs">
							<p class="mb-3 text-muted-foreground">
								These ship with the proxy and apply automatically. Adding an override above
								replaces the matching default; greyed-out rows below are currently shadowed.
							</p>
							{#snippet defaultsTable(label: string, entries: BedrockModelMapEntry[], shadowed: Set<string>)}
								{#if entries.length > 0}
									<div class="mb-3">
										<div class="mb-1 text-[11px] font-semibold uppercase text-muted-foreground">
											{label}
										</div>
										<div class="rounded border bg-background">
											{#each entries as e (e.id)}
												{@const isShadowed = shadowed.has(e.id)}
												<div
													class={'grid grid-cols-12 gap-2 border-b px-2 py-1.5 last:border-b-0 ' +
														(isShadowed ? 'opacity-40 line-through' : '')}
												>
													<span class="col-span-7 truncate font-mono">{e.id}</span>
													<span class="col-span-5 truncate font-mono text-muted-foreground">
														→ {e.tag}
													</span>
												</div>
											{/each}
										</div>
									</div>
								{/if}
							{/snippet}
							{@render defaultsTable('Invoke / Converse', defaults.invoke, invokeOverrideIds)}
							{@render defaultsTable('Embeddings', defaults.embed, embedOverrideIds)}
						</div>
					</details>
				</div>
			{/if}
		</section>

		<!-- SES section -->
		<section class="rounded-lg border bg-card">
			<header class="flex items-start justify-between gap-4 border-b p-4">
				<div>
					<div class="flex items-center gap-2">
						<h2 class="text-base font-semibold">SES outbox retention</h2>
						{#if isSesModified}
							<Badge variant="secondary" class="text-[10px]">modified</Badge>
						{/if}
					</div>
					<p class="mt-1 text-sm text-muted-foreground">
						Hours to retain captured outbound emails before the hourly sweep deletes them.
						Set to 0 to keep all emails forever.
					</p>
				</div>
				{#if isSesModified}
					<Button variant="ghost" size="sm" onclick={resetSes}>
						<RotateCcwIcon class="h-4 w-4" />
						<span class="ml-1">Reset</span>
					</Button>
				{/if}
			</header>
			<div class="p-4">
				<Label for="ses-retention">Retention hours</Label>
				<Input
					id="ses-retention"
					type="number"
					min="0"
					bind:value={sesRetentionHours}
					class="max-w-xs"
				/>
				<p class="mt-1 text-xs text-muted-foreground">
					Default: 720 (30 days). Sweep runs once per hour.
				</p>
			</div>
		</section>

		<!-- IAM section -->
		<section class="rounded-lg border bg-card">
			<header class="flex items-start justify-between gap-4 border-b p-4">
				<div>
					<div class="flex items-center gap-2">
						<h2 class="text-base font-semibold">IAM enforcement</h2>
						{#if isIamModified}
							<Badge variant="secondary" class="text-[10px]">modified</Badge>
						{/if}
					</div>
					<p class="mt-1 text-sm text-muted-foreground">
						When on, every request runs through the IAM policy engine: identity policies,
						resource policies, SCPs, KMS grants. When off, all calls are allowed regardless
						of identity. Off is the default for ergonomic local dev; flip on to test
						policy logic.
					</p>
				</div>
				<div class="flex items-center gap-2 pt-1">
					{#if isIamModified}
						<Button variant="ghost" size="sm" onclick={resetIam}>
							<RotateCcwIcon class="h-4 w-4" />
							<span class="ml-1">Reset</span>
						</Button>
					{/if}
					<Label for="iam-enforce" class="text-sm">Enforce</Label>
					<Switch id="iam-enforce" bind:checked={iamEnforce} />
				</div>
			</header>
			<div class="p-4 text-xs text-muted-foreground">
				Equivalent CLI flag:
				<code class="ml-1 rounded bg-muted px-1.5 py-0.5 font-mono">AWSIM_IAM_ENFORCE=true</code>
			</div>
		</section>

		<!-- Logging section -->
		<section class="rounded-lg border bg-card">
			<header class="flex items-start justify-between gap-4 border-b p-4">
				<div>
					<div class="flex items-center gap-2">
						<h2 class="text-base font-semibold">Log level</h2>
						{#if isLoggingModified}
							<Badge variant="secondary" class="text-[10px]">modified</Badge>
						{/if}
					</div>
					<p class="mt-1 text-sm text-muted-foreground">
						Tracing filter directive. Same syntax as the <code>RUST_LOG</code> env var:
						<code>info</code>, <code>debug</code>, or per-target overrides like
						<code>info,awsim_dynamodb=debug,sqlx=warn</code>. Hot-reloaded — flip to
						<code>debug</code> to capture more detail without restarting.
					</p>
				</div>
				{#if isLoggingModified}
					<Button variant="ghost" size="sm" onclick={resetLogging}>
						<RotateCcwIcon class="h-4 w-4" />
						<span class="ml-1">Reset</span>
					</Button>
				{/if}
			</header>
			<div class="space-y-2 p-4">
				<div class="flex flex-wrap items-center gap-2">
					<Label for="log-level" class="text-sm shrink-0">Filter</Label>
					<Input
						id="log-level"
						bind:value={logLevel}
						placeholder="info"
						class="max-w-md"
					/>
					<div class="flex flex-wrap gap-1">
						{#each ['error', 'warn', 'info', 'debug', 'trace'] as preset (preset)}
							<Button
								variant="outline"
								size="sm"
								onclick={() => (logLevel = preset)}
							>
								{preset}
							</Button>
						{/each}
					</div>
				</div>
			</div>
		</section>

		<!-- Restart-required values -->
		<section class="rounded-lg border bg-card">
			<header class="border-b p-4">
				<h2 class="text-base font-semibold">Restart required</h2>
				<p class="mt-1 text-sm text-muted-foreground">
					These settings are baked in at startup. Pass the matching CLI flag and restart awsim to change them.
				</p>
			</header>
			<div class="p-4 text-sm">
				<ul class="space-y-1 text-muted-foreground">
					<li>
						<Badge variant="outline" class="mr-2 font-mono text-[11px]">--port</Badge> Server listen port
					</li>
					<li>
						<Badge variant="outline" class="mr-2 font-mono text-[11px]">--region</Badge>
						<Badge variant="outline" class="mr-2 font-mono text-[11px]">--account-id</Badge>
						Default AWS coordinates
					</li>
					<li>
						<Badge variant="outline" class="mr-2 font-mono text-[11px]">--data-dir</Badge> Persistence directory
					</li>
					<li>
						<Badge variant="outline" class="mr-2 font-mono text-[11px]">--max-concurrent-requests</Badge>
						Inflight cap (load shedding)
					</li>
					<li>
						<Badge variant="outline" class="mr-2 font-mono text-[11px]">--max-body-bytes</Badge> Request body cap
					</li>
				</ul>
			</div>
		</section>

		<!-- Footer with persistence info -->
		{#if envelope}
			<footer class="flex items-center gap-2 border-t pt-4 text-xs text-muted-foreground">
				<HardDriveIcon class="h-3.5 w-3.5" />
				{#if envelope.persistent && envelope.configPath}
					<span>
						Persisted at
						<code class="rounded bg-muted px-1.5 py-0.5 font-mono">{envelope.configPath}</code>
					</span>
				{:else}
					<span>In-memory only — pass <code class="font-mono">--data-dir</code> to persist.</span>
				{/if}
			</footer>
		{/if}
	</div>
</ServicePage>
