<script lang="ts">
	// Phase 0 scaffold for the Model Gateway. Hosts the bundled
	// provider catalog browser (proves the /_awsim/gateway/catalog
	// endpoint round-trips end-to-end) plus stub tabs that fill in
	// during Phases 1-7.
	import { onMount } from 'svelte';
	import { ServicePage, EmptyState } from '$lib/components/service';
	import {
		Tabs,
		TabsList,
		TabsTrigger,
		TabsContent,
	} from '$lib/components/ui/tabs';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { useTab } from '$lib/util/tab.svelte';
	import { route } from '$lib/url';
	import { getGatewayCatalog, type CatalogProvider, type ProviderCatalog } from '$lib/api/gateway';
	import type { Component } from 'svelte';
	import { toast } from 'svelte-sonner';

	import Server from '@lucide/svelte/icons/server';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Zap from '@lucide/svelte/icons/zap';
	import Flame from '@lucide/svelte/icons/flame';
	import RouteIcon from '@lucide/svelte/icons/route';
	import Cloud from '@lucide/svelte/icons/cloud';
	import Settings from '@lucide/svelte/icons/settings';
	import Network from '@lucide/svelte/icons/network';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import Activity from '@lucide/svelte/icons/activity';
	import Boxes from '@lucide/svelte/icons/boxes';
	import SquareTerminal from '@lucide/svelte/icons/square-terminal';
	import GitFork from '@lucide/svelte/icons/git-fork';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import SearchIcon from '@lucide/svelte/icons/search';

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

	let active: string = $state(
		useTab(
			'gateway',
			['providers', 'credentials', 'models', 'routing', 'health', 'playground'] as const,
			'providers',
			{
				get: (): string => active,
				set: (v) => (active = v),
			},
		),
	);

	let catalog = $state<ProviderCatalog | null>(null);
	let loading = $state(true);
	let query = $state('');
	let kindFilter = $state<'all' | 'local' | 'hosted' | 'aws' | 'custom'>('all');

	onMount(async () => {
		try {
			catalog = await getGatewayCatalog();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load catalog');
		} finally {
			loading = false;
		}
	});

	let filteredProviders = $derived.by<CatalogProvider[]>(() => {
		if (!catalog) return [];
		const q = query.trim().toLowerCase();
		return catalog.providers.filter((p) => {
			if (kindFilter !== 'all' && p.kind !== kindFilter) return false;
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
</script>

<ServicePage
	title="Model Gateway"
	description="Provider-agnostic proxy in front of Bedrock InvokeModel / Converse / embeddings. Manage providers, credentials, model aliases, routing, and health here."
>
	<Tabs bind:value={active} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="providers">
				<Network class="mr-2 h-4 w-4" />Providers
			</TabsTrigger>
			<TabsTrigger value="credentials">
				<KeyRound class="mr-2 h-4 w-4" />Credentials
			</TabsTrigger>
			<TabsTrigger value="models">
				<Boxes class="mr-2 h-4 w-4" />Models &amp; Aliases
			</TabsTrigger>
			<TabsTrigger value="routing">
				<GitFork class="mr-2 h-4 w-4" />Routing
			</TabsTrigger>
			<TabsTrigger value="health">
				<Activity class="mr-2 h-4 w-4" />Health
			</TabsTrigger>
			<TabsTrigger value="playground">
				<SquareTerminal class="mr-2 h-4 w-4" />Playground
			</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<!-- Providers: catalog browser (Phase 0 scope). -->
			<TabsContent value="providers" class="m-0">
				<div class="space-y-4 p-4">
					<section class="rounded-lg border bg-card p-4 text-sm">
						<div class="flex items-start gap-3">
							<Network class="mt-0.5 h-5 w-5 shrink-0 text-muted-foreground" />
							<div class="space-y-1">
								<p class="font-semibold">Bundled provider catalog</p>
								<p class="text-muted-foreground">
									Built-in templates for popular LLM backends. The "Add backend" wizard
									(Phase 2) uses these to pre-fill endpoints, auth fields, and model lists.
									For now, you can still configure the proxy on the
									<a class="underline" href={route('/settings')}>Settings page</a>.
								</p>
							</div>
						</div>
					</section>

					<div class="flex flex-wrap items-center gap-2">
						<div class="relative max-w-xs flex-1">
							<SearchIcon
								class="pointer-events-none absolute left-2 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
							/>
							<Input
								bind:value={query}
								placeholder="Search providers or model ids…"
								class="pl-8"
							/>
						</div>
						{#each ['all', 'local', 'hosted', 'aws', 'custom'] as const as k (k)}
							<Button
								variant={kindFilter === k ? 'default' : 'outline'}
								size="sm"
								onclick={() => (kindFilter = k)}
							>
								{k}
							</Button>
						{/each}
					</div>

					{#if loading}
						<EmptyState icon={Network} title="Loading catalog…" />
					{:else if !catalog || catalog.providers.length === 0}
						<EmptyState
							icon={Network}
							title="No providers in catalog"
							description="The bundled catalog appears empty. This is a build-time issue; please file a bug."
						/>
					{:else}
						<div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
							{#each filteredProviders as p (p.key)}
								{@const Icon = iconFor(p.icon)}
								{@const chatCount = p.models.filter((m) => m.kind === 'chat').length}
								{@const embedCount = p.models.filter((m) => m.kind === 'embed').length}
								<div class="flex flex-col gap-2 rounded-lg border bg-card p-4">
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
									<div class="space-y-1 text-xs">
										<div class="flex items-baseline gap-2">
											<span class="text-muted-foreground">Endpoint</span>
											<code class="truncate font-mono">{p.endpoint_template}</code>
										</div>
										<div class="flex items-baseline gap-2">
											<span class="text-muted-foreground">Auth</span>
											<span class="font-mono">
												{p.auth}
												{#if p.env_hint}
													· <span class="text-muted-foreground">${p.env_hint}</span>
												{/if}
											</span>
										</div>
									</div>
									{#if p.notes}
										<p class="text-xs text-muted-foreground">{p.notes}</p>
									{/if}
									<div class="flex items-center gap-2 pt-1 text-xs">
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
							{:else}
								<EmptyState
									icon={SearchIcon}
									title="No providers match"
									description="Try a different search or filter."
								/>
							{/each}
						</div>
					{/if}
				</div>
			</TabsContent>

			<TabsContent value="credentials" class="m-0">
				<div class="p-4">
					<EmptyState
						icon={KeyRound}
						title="Coming in Phase 1"
						description="Reusable API-key credentials referenced by multiple backends. Today, credentials live inline on each backend; Settings is still the place to edit them."
					/>
				</div>
			</TabsContent>

			<TabsContent value="models" class="m-0">
				<div class="p-4">
					<EmptyState
						icon={Boxes}
						title="Coming in Phase 3"
						description="Map Bedrock model ids to ordered lists of backend targets (primary + fallbacks), with drag-to-reorder. The existing read-only mapping view lives on the Bedrock page."
					/>
				</div>
			</TabsContent>

			<TabsContent value="routing" class="m-0">
				<div class="p-4">
					<EmptyState
						icon={GitFork}
						title="Coming in Phase 4-5"
						description="Automatic fallback on 5xx / timeout / rate-limit, per-target overrides (timeout, max tokens, temperature), and routing strategy (first / round-robin / least-latency)."
					/>
				</div>
			</TabsContent>

			<TabsContent value="health" class="m-0">
				<div class="p-4">
					<EmptyState
						icon={Activity}
						title="Coming in Phase 4"
						description="Background pings every ~30s mark each backend Healthy / Degraded / Down. Use the existing one-shot Check on the Bedrock proxy page in the meantime."
					/>
				</div>
			</TabsContent>

			<TabsContent value="playground" class="m-0">
				<div class="p-4">
					<EmptyState
						icon={SquareTerminal}
						title="Use the existing Bedrock playground"
						description="The in-place per-row tester lands in Phase 7. For now, the full playground lives under Bedrock."
						action={playgroundAction}
					/>
				</div>
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

{#snippet playgroundAction()}
	<Button variant="outline" size="sm" href={route('/bedrock') + '?tab=playground'}>
		Open Bedrock playground
	</Button>
{/snippet}
