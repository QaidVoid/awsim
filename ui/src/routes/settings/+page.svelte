<script lang="ts">
	import { onMount } from 'svelte';
	import {
		getRuntimeConfig,
		putRuntimeConfig,
		type BedrockBackendSpec,
		type ModelMapEntry,
		type RuntimeConfig,
		type RuntimeConfigEnvelope,
	} from '$lib/api/runtime-config';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Switch } from '$lib/components/ui/switch';
	import { Badge } from '$lib/components/ui/badge';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import SaveIcon from '@lucide/svelte/icons/save';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import HardDriveIcon from '@lucide/svelte/icons/hard-drive';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
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
	let loading = $state(true);
	let saving = $state(false);

	let bedrockEnabled = $state(false);
	let defaultBackend = $state('');
	let backendRows = $state<BackendRow[]>([]);
	let invokeRows = $state<MapRow[]>([]);
	let embedRows = $state<MapRow[]>([]);
	let sesRetentionHours = $state(720);

	onMount(load);

	async function load() {
		loading = true;
		try {
			envelope = await getRuntimeConfig();
			seedFromEnvelope(envelope.config);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load runtime config');
		} finally {
			loading = false;
		}
	}

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
		backendRows = backendRows.filter((_, idx) => idx !== i);
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
					<h2 class="text-base font-semibold">Bedrock proxy</h2>
					<p class="mt-1 text-sm text-muted-foreground">
						OpenAI-compatible backends serving Bedrock InvokeModel / Converse / embeddings.
						Disable to fall back to canned responses.
					</p>
				</div>
				<div class="flex items-center gap-2 pt-1">
					<Label for="bedrock-enabled" class="text-sm">Enabled</Label>
					<Switch id="bedrock-enabled" bind:checked={bedrockEnabled} />
				</div>
			</header>

			<div class="space-y-6 p-4">
				<div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
					<div>
						<Label for="default-backend">Default backend</Label>
						<Input
							id="default-backend"
							placeholder="e.g. ollama"
							bind:value={defaultBackend}
							list="backend-name-options"
						/>
						<datalist id="backend-name-options">
							{#each backendNames as n (n)}
								<option value={n}></option>
							{/each}
						</datalist>
						<p class="mt-1 text-xs text-muted-foreground">
							Bare-tag mappings route here when no backend is pinned on the entry.
						</p>
					</div>
				</div>

				<div>
					<div class="mb-2 flex items-center justify-between">
						<h3 class="text-sm font-semibold">Backends</h3>
						<Button variant="outline" size="sm" onclick={addBackend}>
							<PlusIcon class="h-4 w-4" />
							<span class="ml-1">Add</span>
						</Button>
					</div>
					<div class="space-y-2">
						{#each backendRows as row, i (i)}
							<div class="grid grid-cols-12 items-end gap-2 rounded border p-3">
								<div class="col-span-12 sm:col-span-3">
									<Label class="text-xs">Name</Label>
									<Input bind:value={row.name} placeholder="ollama" />
								</div>
								<div class="col-span-12 sm:col-span-5">
									<Label class="text-xs">Endpoint</Label>
									<Input bind:value={row.endpoint} placeholder="http://host:port/v1" />
								</div>
								<div class="col-span-6 sm:col-span-2">
									<Label class="text-xs">Auth</Label>
									<select
										bind:value={row.keyMode}
										class="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
									>
										<option value="none">None</option>
										<option value="inline">Inline key</option>
										<option value="env">Env var</option>
									</select>
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
									<Label class="text-xs">Backend (optional)</Label>
									<Input bind:value={row.backend} list="backend-name-options" placeholder="ollama" />
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
									<Label class="text-xs">Backend (optional)</Label>
									<Input bind:value={row.backend} list="backend-name-options" placeholder="ollama" />
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
		</section>

		<!-- SES section -->
		<section class="rounded-lg border bg-card">
			<header class="border-b p-4">
				<h2 class="text-base font-semibold">SES outbox retention</h2>
				<p class="mt-1 text-sm text-muted-foreground">
					Hours to retain captured outbound emails before the hourly sweep deletes them.
					Set to 0 to keep all emails forever.
				</p>
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
	</div>
</ServicePage>
