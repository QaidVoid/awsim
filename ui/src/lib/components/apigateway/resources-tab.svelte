<script lang="ts">
	import {
		getResources,
		getMethod,
		createResource,
		deleteResource,
		putMethod,
		deleteMethod,
		putIntegration,
		deleteIntegration,
		type Resource,
		type Method,
	} from '$lib/api/apigateway';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle,
	} from '$lib/components/ui/dialog';
	import { toast } from 'svelte-sonner';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';

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

	let createDialogOpen = $state(false);
	let createParentId = $state<string>('');
	let createPathPart = $state('');
	let creating = $state(false);

	let methodDialogOpen = $state(false);
	let methodResource = $state<Resource | null>(null);
	let newMethodHttp = $state<'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH'>('GET');
	let newIntegrationType = $state<'MOCK' | 'HTTP_PROXY' | 'AWS_PROXY' | 'AWS' | 'HTTP'>('MOCK');
	let newIntegrationUri = $state('');
	let savingMethod = $state(false);

	const HTTP_METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE'] as const;
	const INTEGRATION_TYPES = ['MOCK', 'HTTP_PROXY', 'AWS_PROXY', 'AWS', 'HTTP'] as const;

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

	function openCreateDialog(parentId: string) {
		createParentId = parentId;
		createPathPart = '';
		createDialogOpen = true;
	}

	async function submitCreate(e: Event) {
		e.preventDefault();
		if (!createPathPart.trim()) return;
		creating = true;
		try {
			await createResource(restApiId, createParentId, createPathPart.trim());
			toast.success(`Resource ${createPathPart.trim()} created`);
			createDialogOpen = false;
			await load();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Create failed');
		} finally {
			creating = false;
		}
	}

	async function removeResource(res: Resource) {
		if (!confirm(`Delete resource ${res.path}?`)) return;
		try {
			await deleteResource(restApiId, res.id);
			toast.success(`Resource ${res.path} deleted`);
			await load();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		}
	}

	function openMethodDialog(res: Resource) {
		methodResource = res;
		newMethodHttp = 'GET';
		newIntegrationType = 'MOCK';
		newIntegrationUri = '';
		methodDialogOpen = true;
	}

	async function submitMethod(e: Event) {
		e.preventDefault();
		if (!methodResource) return;
		savingMethod = true;
		try {
			await putMethod(restApiId, methodResource.id, newMethodHttp, {
				authorizationType: 'NONE',
				apiKeyRequired: false,
			});
			await putIntegration(restApiId, methodResource.id, newMethodHttp, {
				type: newIntegrationType,
				uri: newIntegrationUri.trim(),
			});
			toast.success(`${newMethodHttp} method added`);
			methodDialogOpen = false;
			await load();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Save failed');
		} finally {
			savingMethod = false;
		}
	}

	async function removeMethod(res: Resource, http: string) {
		if (!confirm(`Delete ${http} on ${res.path}?`)) return;
		try {
			await deleteIntegration(restApiId, res.id, http).catch(() => undefined);
			await deleteMethod(restApiId, res.id, http);
			toast.success(`${http} deleted`);
			await load();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		}
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
					<div class="flex w-full items-center gap-2 px-3 py-2">
						<button
							type="button"
							onclick={() => toggleResource(res)}
							class="flex flex-1 items-center gap-2 text-left text-sm transition-colors hover:text-foreground"
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
							<span class="ml-2 flex items-center gap-1">
								{#each res.resourceMethods as m (m)}
									<Badge variant="outline" class="h-4 px-1 text-[10px] {methodColor(m)}">
										{m}
									</Badge>
								{/each}
							</span>
						</button>
						<Button
							variant="ghost"
							size="sm"
							class="h-6 gap-1 px-1.5"
							title="Add child resource"
							onclick={() => openCreateDialog(res.id)}
						>
							<Plus class="size-3.5" />
						</Button>
						<Button
							variant="ghost"
							size="sm"
							class="h-6 gap-1 px-1.5"
							title="Add method"
							onclick={() => openMethodDialog(res)}
						>
							<span class="font-mono text-[10px]">+M</span>
						</Button>
						{#if res.path !== '/'}
							<Button
								variant="ghost"
								size="sm"
								class="h-6 gap-1 px-1.5 text-destructive"
								title="Delete resource"
								onclick={() => removeResource(res)}
							>
								<Trash2 class="size-3.5" />
							</Button>
						{/if}
					</div>
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
											<Button
												variant="ghost"
												size="sm"
												class="ml-auto h-5 gap-1 px-1 text-destructive"
												onclick={() => removeMethod(res, m)}
											>
												<Trash2 class="size-3" />
											</Button>
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

<Dialog bind:open={createDialogOpen}>
	<DialogContent>
		<DialogHeader>
			<DialogTitle>Create child resource</DialogTitle>
			<DialogDescription>
				Append a path segment under the selected parent resource.
			</DialogDescription>
		</DialogHeader>
		<form onsubmit={submitCreate} class="space-y-3">
			<div class="space-y-1">
				<Label for="cr-path">Path part</Label>
				<Input
					id="cr-path"
					bind:value={createPathPart}
					placeholder="users"
					class="font-mono"
					required
				/>
				<p class="text-[11px] text-muted-foreground">
					Single segment without leading slash, e.g. <code>users</code> or
					<code>{`{userId}`}</code> for path params.
				</p>
			</div>
			<DialogFooter>
				<Button type="submit" disabled={creating || !createPathPart.trim()}>
					{creating ? 'Creating...' : 'Create'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

<Dialog bind:open={methodDialogOpen}>
	<DialogContent>
		<DialogHeader>
			<DialogTitle>
				Add method on {methodResource?.path ?? '/'}
			</DialogTitle>
			<DialogDescription>
				Configure the HTTP method and a backend integration. MOCK and
				AWS_PROXY (Lambda) are dispatched at runtime via the Route Tester
				tab; HTTP / HTTP_PROXY return 501 until an outbound HTTP client is
				wired.
			</DialogDescription>
		</DialogHeader>
		<form onsubmit={submitMethod} class="space-y-3">
			<div class="grid grid-cols-2 gap-3">
				<div class="space-y-1">
					<Label for="mm-http">HTTP method</Label>
					<select
						id="mm-http"
						bind:value={newMethodHttp}
						class="h-9 w-full rounded-md border border-border bg-background px-2 text-sm"
					>
						{#each HTTP_METHODS as m (m)}
							<option value={m}>{m}</option>
						{/each}
					</select>
				</div>
				<div class="space-y-1">
					<Label for="mm-int">Integration type</Label>
					<select
						id="mm-int"
						bind:value={newIntegrationType}
						class="h-9 w-full rounded-md border border-border bg-background px-2 text-sm"
					>
						{#each INTEGRATION_TYPES as t (t)}
							<option value={t}>{t}</option>
						{/each}
					</select>
				</div>
			</div>
			{#if newIntegrationType !== 'MOCK'}
				<div class="space-y-1">
					<Label for="mm-uri">Integration URI</Label>
					<Input
						id="mm-uri"
						bind:value={newIntegrationUri}
						placeholder="https://example.com/path or arn:aws:lambda:..."
						class="font-mono text-xs"
					/>
				</div>
			{/if}
			<DialogFooter>
				<Button type="submit" disabled={savingMethod}>
					{savingMethod ? 'Saving...' : 'Save method'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
