<script lang="ts">
	// Configured backends, with a provider-aware Add wizard built on
	// the bundled catalog. Each action (add/edit/remove/default)
	// commits immediately via PUT /_awsim/runtime-config so the
	// in-progress state never drifts from what the server enforces.
	import { onMount, onDestroy } from 'svelte';
	import {
		getRuntimeConfig,
		putRuntimeConfig,
		type BedrockBackendSpec,
		type BedrockSpec,
		type RuntimeConfig,
		type RuntimeConfigEnvelope,
	} from '$lib/api/runtime-config';
	import {
		getGatewayCatalog,
		getGatewayHealth,
		type BackendHealth,
		type BackendStatus,
		type CatalogProvider,
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
	import BackendWizardDialog from './backend-wizard-dialog.svelte';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import StarIcon from '@lucide/svelte/icons/star';
	import NetworkIcon from '@lucide/svelte/icons/network';
	import SearchIcon from '@lucide/svelte/icons/search';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import type { Component } from 'svelte';
	import Server from '@lucide/svelte/icons/server';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Zap from '@lucide/svelte/icons/zap';
	import Flame from '@lucide/svelte/icons/flame';
	import RouteIcon from '@lucide/svelte/icons/route';
	import Cloud from '@lucide/svelte/icons/cloud';
	import Settings from '@lucide/svelte/icons/settings';
	import { toast } from 'svelte-sonner';

	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	const ICON_MAP: Record<string, Component<any>> = {
		server: Server,
		sparkles: Sparkles,
		zap: Zap,
		flame: Flame,
		route: RouteIcon,
		cloud: Cloud,
		settings: Settings,
	};

	function iconFor(name: string) {
		return ICON_MAP[name] ?? Server;
	}

	let envelope = $state<RuntimeConfigEnvelope | null>(null);
	let catalog = $state<ProviderCatalog | null>(null);
	let loading = $state(true);
	let savingAction = $state(false);
	let healthByBackend = $state<Record<string, BackendHealth>>({});
	let healthTimer: ReturnType<typeof setInterval> | null = null;

	let wizardOpen = $state(false);
	let wizardMode = $state<'add' | 'edit'>('add');
	let wizardInitial = $state<{ name: string; spec: BedrockBackendSpec } | null>(null);

	let catalogQuery = $state('');
	let catalogKindFilter = $state<'all' | 'local' | 'hosted' | 'aws' | 'custom'>('all');
	let catalogOpen = $state(false);

	onMount(async () => {
		try {
			const [env, cat] = await Promise.all([getRuntimeConfig(), getGatewayCatalog()]);
			envelope = env;
			catalog = cat;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load backends');
		} finally {
			loading = false;
		}
		void refreshHealth();
		healthTimer = setInterval(refreshHealth, 5000);
	});

	onDestroy(() => {
		if (healthTimer !== null) clearInterval(healthTimer);
	});

	async function refreshHealth() {
		try {
			const res = await getGatewayHealth();
			const next: Record<string, BackendHealth> = {};
			for (const b of res.backends) next[b.backend] = b;
			healthByBackend = next;
		} catch {
			// Silent: health poll failure shouldn't spam the user.
		}
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

	let backends = $derived.by<Array<{ name: string; spec: BedrockBackendSpec }>>(() => {
		const map = envelope?.config.bedrock.spec.backends ?? {};
		return Object.entries(map)
			.map(([name, spec]) => ({ name, spec }))
			.sort((a, b) => a.name.localeCompare(b.name));
	});

	let backendNames = $derived(backends.map((b) => b.name));
	let credentialNames = $derived.by<string[]>(() => {
		const creds = envelope?.config.bedrock.spec.credentials ?? {};
		return Object.keys(creds).sort();
	});

	let defaultBackend = $derived(envelope?.config.bedrock.spec.default_backend ?? null);

	function providerFor(spec: BedrockBackendSpec): CatalogProvider | null {
		if (!catalog || !spec.provider) return null;
		return catalog.providers.find((p) => p.key === spec.provider) ?? null;
	}

	function authSummary(spec: BedrockBackendSpec): { label: string; detail?: string } {
		if (spec.credential) return { label: 'credential', detail: spec.credential };
		if (spec.api_key) return { label: 'inline key' };
		if (spec.api_key_env) return { label: 'env var', detail: `$${spec.api_key_env}` };
		return { label: 'none' };
	}

	function openAdd() {
		wizardMode = 'add';
		wizardInitial = null;
		wizardOpen = true;
	}

	function openEdit(b: { name: string; spec: BedrockBackendSpec }) {
		wizardMode = 'edit';
		// Deep-copy so the wizard's edits don't mutate the live
		// envelope until the user saves.
		wizardInitial = { name: b.name, spec: { ...b.spec } };
		wizardOpen = true;
	}

	function nextSpec(mutate: (spec: BedrockSpec) => void): RuntimeConfig | null {
		if (!envelope) return null;
		const cloned: BedrockSpec = JSON.parse(JSON.stringify(envelope.config.bedrock.spec));
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

	async function handleWizardSubmit(result: { name: string; spec: BedrockBackendSpec }) {
		const oldName = wizardMode === 'edit' && wizardInitial ? wizardInitial.name : null;
		await applyMutation((spec) => {
			if (oldName && oldName !== result.name) {
				delete spec.backends[oldName];
				// Rename also follows through to default + mapping
				// `routed` entries so the user doesn't end up with a
				// dangling reference on a rename.
				if (spec.default_backend === oldName) {
					spec.default_backend = result.name;
				}
				for (const key of ['invoke', 'embed'] as const) {
					for (const [id, entry] of Object.entries(spec[key])) {
						if (typeof entry === 'object' && entry.backend === oldName) {
							spec[key][id] = { ...entry, backend: result.name };
						}
					}
				}
			}
			spec.backends[result.name] = result.spec;
			// Promote first-ever backend to the default automatically.
			if (!spec.default_backend && Object.keys(spec.backends).length === 1) {
				spec.default_backend = result.name;
			}
		}, oldName ? 'Backend updated' : 'Backend added');
	}

	async function remove(b: { name: string; spec: BedrockBackendSpec }) {
		// Server will also reject if mappings still reference this
		// backend; checking first gives a friendlier message.
		const spec = envelope?.config.bedrock.spec;
		if (spec) {
			const refs: string[] = [];
			for (const k of ['invoke', 'embed'] as const) {
				for (const [id, entry] of Object.entries(spec[k])) {
					if (typeof entry === 'object' && entry.backend === b.name) {
						refs.push(`${k}:${id}`);
					}
				}
			}
			if (refs.length > 0) {
				toast.error(
					`Backend '${b.name}' is still referenced by ${refs.length} mapping${refs.length === 1 ? '' : 's'} (${refs.slice(0, 3).join(', ')}${refs.length > 3 ? '…' : ''}). Drop those first.`,
				);
				return;
			}
		}
		if (!confirm(`Remove backend '${b.name}'?`)) return;
		await applyMutation((s) => {
			delete s.backends[b.name];
			if (s.default_backend === b.name) {
				const remaining = Object.keys(s.backends);
				s.default_backend = remaining[0] ?? null;
			}
		}, 'Backend removed');
	}

	async function makeDefault(name: string) {
		if (name === defaultBackend) return;
		await applyMutation((s) => {
			s.default_backend = name;
		}, `Default backend set to ${name}`);
	}

	async function clearDefault() {
		await applyMutation((s) => {
			s.default_backend = null;
		}, 'Default backend cleared');
	}

	let filteredCatalog = $derived.by<CatalogProvider[]>(() => {
		if (!catalog) return [];
		const q = catalogQuery.trim().toLowerCase();
		return catalog.providers.filter((p) => {
			if (catalogKindFilter !== 'all' && p.kind !== catalogKindFilter) return false;
			if (!q) return true;
			return (
				p.name.toLowerCase().includes(q) ||
				p.key.toLowerCase().includes(q) ||
				p.models.some((m) => m.id.toLowerCase().includes(q))
			);
		});
	});

	function kindBadgeVariant(kind: CatalogProvider['kind']): 'default' | 'secondary' | 'outline' {
		if (kind === 'aws') return 'default';
		if (kind === 'hosted') return 'secondary';
		return 'outline';
	}

	function statusDotClass(status: BackendStatus | undefined): string {
		switch (status) {
			case 'healthy':
				return 'bg-emerald-500';
			case 'degraded':
				return 'bg-amber-500';
			case 'down':
				return 'bg-rose-500';
			case 'unknown':
			default:
				return 'bg-muted-foreground/40';
		}
	}

	function statusLabel(status: BackendStatus | undefined): string {
		switch (status) {
			case 'healthy':
				return 'Healthy';
			case 'degraded':
				return 'Degraded';
			case 'down':
				return 'Down';
			case 'unknown':
			default:
				return 'No probe yet';
		}
	}
</script>

<div class="space-y-4 p-4">
	<header class="flex items-center justify-between">
		<div>
			<h2 class="text-lg font-semibold">Backends</h2>
			<p class="text-sm text-muted-foreground">
				{backends.length} configured backend{backends.length === 1 ? '' : 's'}
				{#if defaultBackend}
					· default: <span class="font-mono">{defaultBackend}</span>
				{:else}
					· no default
				{/if}
			</p>
		</div>
		<div class="flex gap-2">
			<Button variant="ghost" size="sm" onclick={reload} disabled={loading || savingAction}>
				<RefreshCwIcon class={loading ? 'h-4 w-4 animate-spin' : 'h-4 w-4'} />
				<span class="ml-2">Reload</span>
			</Button>
			<Button size="sm" onclick={openAdd} disabled={!catalog || savingAction}>
				<PlusIcon class="h-4 w-4" />
				<span class="ml-2">Add backend</span>
			</Button>
		</div>
	</header>

	{#if loading && backends.length === 0}
		<EmptyState icon={NetworkIcon} title="Loading backends…" />
	{:else if backends.length === 0}
		<EmptyState
			icon={NetworkIcon}
			title="No backends yet"
			description="Add a backend to route Bedrock invocations to a real LLM. Pick from the bundled provider catalog (Ollama, OpenAI, Groq, …) or wire a Custom OpenAI-compatible endpoint."
		/>
	{:else}
		<div class="rounded-lg border bg-card">
			<Table>
				<TableHeader>
					<TableRow>
						<TableHead class="w-12"></TableHead>
						<TableHead>Name</TableHead>
						<TableHead>Provider</TableHead>
						<TableHead>Endpoint</TableHead>
						<TableHead>Auth</TableHead>
						<TableHead class="text-right">Actions</TableHead>
					</TableRow>
				</TableHeader>
				<TableBody>
					{#each backends as b (b.name)}
						{@const prov = providerFor(b.spec)}
						{@const Icon = iconFor(prov?.icon ?? 'settings')}
						{@const isDefault = b.name === defaultBackend}
						{@const auth = authSummary(b.spec)}
						{@const h = healthByBackend[b.name]}
						<TableRow>
							<TableCell>
								<Button
									variant="ghost"
									size="icon"
									onclick={() => (isDefault ? clearDefault() : makeDefault(b.name))}
									disabled={savingAction}
									aria-label={isDefault ? 'Clear default' : 'Set default'}
									title={isDefault ? 'This is the default backend (click to clear)' : 'Make default'}
								>
									<StarIcon
										class={isDefault
											? 'h-4 w-4 fill-amber-500 text-amber-500'
											: 'h-4 w-4 text-muted-foreground'}
									/>
								</Button>
							</TableCell>
							<TableCell>
								<div class="flex items-center gap-2">
									<span
										class={'inline-block h-2 w-2 rounded-full ' + statusDotClass(h?.status)}
										title={statusLabel(h?.status) +
											(h?.lastLatencyMs !== undefined && h?.lastLatencyMs !== null
												? ` · ${h.lastLatencyMs}ms`
												: '')}
									></span>
									<Icon class="h-4 w-4 text-muted-foreground" />
									<span class="font-mono text-sm">{b.name}</span>
									{#if isDefault}
										<Badge variant="default" class="text-[10px] uppercase">default</Badge>
									{/if}
								</div>
							</TableCell>
							<TableCell>
								{#if prov}
									<Badge variant={kindBadgeVariant(prov.kind)} class="text-[10px] uppercase">
										{prov.name}
									</Badge>
								{:else if b.spec.provider}
									<Badge variant="outline" class="text-[10px] uppercase">
										{b.spec.provider}
									</Badge>
								{:else}
									<Badge variant="outline" class="text-[10px] uppercase">custom</Badge>
								{/if}
							</TableCell>
							<TableCell>
								<code class="text-xs">{b.spec.endpoint}</code>
							</TableCell>
							<TableCell>
								<div class="flex flex-col gap-0.5">
									<span class="text-xs">{auth.label}</span>
									{#if auth.detail}
										<code class="text-[10px] text-muted-foreground">{auth.detail}</code>
									{/if}
								</div>
							</TableCell>
							<TableCell class="text-right">
								<div class="flex justify-end gap-1">
									<Button
										variant="ghost"
										size="icon"
										onclick={() => openEdit(b)}
										disabled={savingAction}
										aria-label="Edit"
									>
										<PencilIcon class="h-4 w-4" />
									</Button>
									<Button
										variant="ghost"
										size="icon"
										onclick={() => remove(b)}
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

	<section class="rounded-lg border bg-card">
		<button
			type="button"
			class="flex w-full items-center justify-between p-4 text-left"
			onclick={() => (catalogOpen = !catalogOpen)}
		>
			<div>
				<h3 class="text-sm font-semibold">Browse provider catalog</h3>
				<p class="text-xs text-muted-foreground">
					Read-only reference of the bundled providers + their well-known models.
				</p>
			</div>
			<span class="text-xs text-muted-foreground">{catalogOpen ? 'hide' : 'show'}</span>
		</button>
		{#if catalogOpen}
			<div class="space-y-3 border-t p-4">
				<div class="flex flex-wrap items-center gap-2">
					<div class="relative max-w-xs flex-1">
						<SearchIcon
							class="pointer-events-none absolute left-2 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
						/>
						<Input
							bind:value={catalogQuery}
							placeholder="Search providers or model ids…"
							class="pl-8"
						/>
					</div>
					{#each ['all', 'local', 'hosted', 'aws', 'custom'] as const as k (k)}
						<Button
							variant={catalogKindFilter === k ? 'default' : 'outline'}
							size="sm"
							onclick={() => (catalogKindFilter = k)}
						>
							{k}
						</Button>
					{/each}
				</div>

				<div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
					{#each filteredCatalog as p (p.key)}
						{@const Icon = iconFor(p.icon)}
						{@const chatCount = p.models.filter((m) => m.kind === 'chat').length}
						{@const embedCount = p.models.filter((m) => m.kind === 'embed').length}
						<div class="flex flex-col gap-2 rounded-lg border bg-background p-3 text-sm">
							<div class="flex items-start justify-between gap-2">
								<div class="flex items-start gap-3">
									<Icon class="mt-0.5 h-5 w-5 text-muted-foreground" />
									<div>
										<div class="font-semibold leading-tight">{p.name}</div>
										<div class="font-mono text-xs text-muted-foreground">{p.key}</div>
									</div>
								</div>
								<Badge variant={kindBadgeVariant(p.kind)} class="text-[10px] uppercase">
									{p.kind}
								</Badge>
							</div>
							<div class="font-mono text-xs text-muted-foreground">
								{p.endpoint_template}
							</div>
							<div class="flex items-center gap-2 text-xs">
								<Badge variant="outline" class="font-mono">
									{chatCount} chat
								</Badge>
								<Badge variant="outline" class="font-mono">
									{embedCount} embed
								</Badge>
								{#if p.docs_url}
									<a
										class="ml-auto inline-flex items-center gap-1 text-muted-foreground hover:text-foreground"
										href={p.docs_url}
										target="_blank"
										rel="noopener"
									>
										docs <ExternalLink class="h-3 w-3" />
									</a>
								{/if}
							</div>
						</div>
					{/each}
				</div>
			</div>
		{/if}
	</section>
</div>

{#if catalog}
	<BackendWizardDialog
		open={wizardOpen}
		mode={wizardMode}
		providers={catalog.providers}
		credentials={credentialNames}
		existingNames={backendNames}
		initial={wizardInitial}
		onOpenChange={(o) => (wizardOpen = o)}
		onSubmit={handleWizardSubmit}
	/>
{/if}
