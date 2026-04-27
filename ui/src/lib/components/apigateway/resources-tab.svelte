<script lang="ts">
	import {
		getResources,
		getMethod,
		type Resource,
		type Method,
	} from '$lib/api/apigateway';
	import { Badge } from '$lib/components/ui/badge';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';

	interface Props {
		restApiId: string;
	}

	let { restApiId }: Props = $props();

	let resources = $state<Resource[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);

	let expanded = $state<Set<string>>(new Set());
	let methodCache = $state<Record<string, Method>>({});
	let methodLoading = $state<Set<string>>(new Set());

	async function load() {
		loading = true;
		error = null;
		expanded = new Set();
		methodCache = {};
		try {
			resources = await getResources(restApiId);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load resources';
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		if (restApiId) load();
	});

	async function toggleResource(res: Resource) {
		const next = new Set(expanded);
		if (next.has(res.id)) {
			next.delete(res.id);
		} else {
			next.add(res.id);
			for (const m of res.resourceMethods) {
				const key = `${res.id}:${m}`;
				if (methodCache[key] || methodLoading.has(key)) continue;
				methodLoading.add(key);
				methodLoading = new Set(methodLoading);
				try {
					const method = await getMethod(restApiId, res.id, m);
					methodCache = { ...methodCache, [key]: method };
				} catch {
					/* ignore */
				} finally {
					methodLoading.delete(key);
					methodLoading = new Set(methodLoading);
				}
			}
		}
		expanded = next;
	}

	function methodColor(http: string): string {
		switch (http.toUpperCase()) {
			case 'GET':
				return 'text-emerald-400';
			case 'POST':
				return 'text-blue-400';
			case 'PUT':
				return 'text-amber-400';
			case 'DELETE':
				return 'text-red-400';
			case 'PATCH':
				return 'text-purple-400';
			default:
				return 'text-muted-foreground';
		}
	}
</script>

<div class="p-4">
	{#if loading}
		<div class="flex h-32 items-center justify-center text-muted-foreground">
			<Loader2 class="size-4 animate-spin" />
		</div>
	{:else if error}
		<div class="text-sm text-destructive">{error}</div>
	{:else if resources.length === 0}
		<div class="text-sm text-muted-foreground">No resources defined.</div>
	{:else}
		<ul class="flex flex-col gap-1">
			{#each resources as res (res.id)}
				<li class="rounded-md border border-border bg-card/40">
					<button
						type="button"
						onclick={() => toggleResource(res)}
						class="flex w-full items-center gap-2 px-3 py-2 text-left text-sm transition-colors hover:bg-muted/40"
					>
						{#if res.resourceMethods.length > 0}
							{#if expanded.has(res.id)}
								<ChevronDown class="size-3.5 text-muted-foreground" />
							{:else}
								<ChevronRight class="size-3.5 text-muted-foreground" />
							{/if}
						{:else}
							<span class="size-3.5"></span>
						{/if}
						<span class="font-mono text-xs">{res.path || '/'}</span>
						<span class="ml-auto flex items-center gap-1">
							{#each res.resourceMethods as m (m)}
								<Badge variant="outline" class="h-4 px-1 text-[10px] {methodColor(m)}">
									{m}
								</Badge>
							{/each}
						</span>
					</button>
					{#if expanded.has(res.id) && res.resourceMethods.length > 0}
						<div class="border-t border-border/40 px-3 py-2">
							<ul class="flex flex-col gap-2">
								{#each res.resourceMethods as m (m)}
									{@const key = `${res.id}:${m}`}
									{@const method = methodCache[key]}
									<li class="rounded border border-border/40 bg-background/40 p-2 text-xs">
										<div class="mb-1 flex items-center gap-2">
											<span class="font-mono font-semibold {methodColor(m)}">{m}</span>
											{#if method}
												<Badge variant="outline" class="h-4 px-1 text-[10px]">
													{method.authorizationType}
												</Badge>
												{#if method.apiKeyRequired}
													<Badge variant="outline" class="h-4 px-1 text-[10px]">
														API key
													</Badge>
												{/if}
											{:else if methodLoading.has(key)}
												<Loader2 class="size-3 animate-spin text-muted-foreground" />
											{/if}
										</div>
										{#if method?.methodIntegration}
											<div class="grid grid-cols-[80px_1fr] gap-x-2 gap-y-0.5 text-muted-foreground">
												<span>Type</span>
												<span class="font-mono">{method.methodIntegration.type}</span>
												<span>Method</span>
												<span class="font-mono">
													{method.methodIntegration.httpMethod || '—'}
												</span>
												<span>URI</span>
												<span class="truncate font-mono">
													{method.methodIntegration.uri || '—'}
												</span>
											</div>
										{/if}
									</li>
								{/each}
							</ul>
						</div>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</div>
