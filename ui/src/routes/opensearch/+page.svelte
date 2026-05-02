<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { onMount } from 'svelte';
	import {
		listIndices,
		createIndex,
		deleteIndex,
		clusterHealth,
		search,
		indexDocument,
		deleteDocument,
		getMapping,
		type IndexSummary,
		type SearchHit,
	} from '$lib/api/opensearch';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle,
	} from '$lib/components/ui/dialog';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import SearchIcon from '@lucide/svelte/icons/search';
	import Play from '@lucide/svelte/icons/play';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import { toast } from 'svelte-sonner';

	let indices = $state<IndexSummary[]>([]);
	let health = $state<{ cluster_name: string; status: string; number_of_nodes: number; active_shards: number } | null>(null);
	let loading = $state(false);

	let createOpen = $state(false);
	let newIndexName = $state('');

	let selected = $state<IndexSummary | null>(null);
	let docs = $state<SearchHit[]>([]);
	let docsLoading = $state(false);
	let total = $state(0);
	let mapping = $state<Record<string, unknown> | null>(null);

	let queryText = $state(
		JSON.stringify({ query: { match_all: {} }, size: 20 }, null, 2)
	);
	let querying = $state(false);
	let queryResult = $state<SearchHit[]>([]);
	let queryTotal = $state(0);

	let putOpen = $state(false);
	let putId = $state('');
	let putBody = $state('{\n  "field": "value"\n}');

	let active: string = $state(
		useTab('opensearch', ['browse', 'query', 'mapping'] as const, 'browse', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);

	onMount(loadAll);

	async function loadAll() {
		loading = true;
		try {
			[indices, health] = await Promise.all([
				listIndices(),
				clusterHealth().catch(() => null),
			]);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load OpenSearch');
		} finally {
			loading = false;
		}
	}

	async function selectIndex(i: IndexSummary) {
		selected = i;
		active = 'browse';
		await Promise.all([reloadDocs(i.index), loadMapping(i.index)]);
	}

	async function loadMapping(name: string) {
		try {
			mapping = (await getMapping(name)) as Record<string, unknown>;
		} catch {
			mapping = null;
		}
	}

	async function reloadDocs(name: string) {
		docsLoading = true;
		try {
			const r = await search(name, { query: { match_all: {} }, size: 25 });
			docs = r.hits.hits;
			total = r.hits.total.value;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Search failed');
		} finally {
			docsLoading = false;
		}
	}

	async function runQuery() {
		if (!selected) return;
		let parsed: unknown;
		try {
			parsed = JSON.parse(queryText);
		} catch {
			toast.error('Query is not valid JSON');
			return;
		}
		querying = true;
		try {
			const r = await search(selected.index, parsed);
			queryResult = r.hits.hits;
			queryTotal = r.hits.total.value;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Search failed');
		} finally {
			querying = false;
		}
	}

	async function handleCreate() {
		const name = newIndexName.trim();
		if (!name) {
			toast.error('Index name is required');
			return;
		}
		try {
			await createIndex(name);
			toast.success(`Created index ${name}`);
			createOpen = false;
			newIndexName = '';
			await loadAll();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Create failed');
		}
	}

	async function handleDeleteIndex(i: IndexSummary) {
		if (!confirm(`Delete index "${i.index}" and all its documents?`)) return;
		try {
			await deleteIndex(i.index);
			toast.success(`Deleted ${i.index}`);
			if (selected?.index === i.index) {
				selected = null;
				docs = [];
			}
			await loadAll();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}

	async function handleDeleteDoc(id: string) {
		if (!selected) return;
		if (!confirm(`Delete document "${id}"?`)) return;
		try {
			await deleteDocument(selected.index, id);
			toast.success(`Deleted ${id}`);
			await reloadDocs(selected.index);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}

	async function savePutDoc() {
		if (!selected || !putId.trim()) {
			toast.error('Document ID is required');
			return;
		}
		let parsed: Record<string, unknown>;
		try {
			parsed = JSON.parse(putBody);
		} catch {
			toast.error('Document is not valid JSON');
			return;
		}
		try {
			await indexDocument(selected.index, putId.trim(), parsed);
			toast.success(`Indexed ${putId}`);
			putOpen = false;
			putId = '';
			await reloadDocs(selected.index);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Index failed');
		}
	}
</script>

{#snippet pageActions()}
	{#if health}
		<Badge variant={health.status === 'green' ? 'default' : 'outline'} class="gap-1">
			<span
				class="size-1.5 rounded-full {health.status === 'green'
					? 'bg-emerald-500'
					: health.status === 'yellow'
						? 'bg-amber-500'
						: 'bg-red-500'}"
			></span>
			{health.cluster_name}
		</Badge>
	{/if}
	<Badge variant="outline" class="font-mono">
		{indices.length} ind{indices.length === 1 ? 'ex' : 'ices'}
	</Badge>
	<Button size="sm" onclick={() => (createOpen = true)}>
		<Plus class="size-3.5" />
		<span class="ml-1">Create index</span>
	</Button>
	<Button variant="ghost" size="icon-sm" onclick={loadAll} disabled={loading} title="Refresh">
		<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
	</Button>
{/snippet}

<ServicePage
	title="OpenSearch"
	description="Elasticsearch-compatible REST API with disk-backed storage via redb."
	actions={pageActions}
>
	<div class="grid h-full min-h-0 grid-cols-[280px_minmax(0,1fr)] divide-x divide-border">
		<!-- Indices list -->
		<aside class="min-h-0 overflow-y-auto">
			{#if indices.length === 0}
				<div class="px-3 py-6 text-center text-xs text-muted-foreground">
					{loading ? 'Loading…' : 'No indices yet. Create one above.'}
				</div>
			{:else}
				<ul class="flex flex-col">
					{#each indices as i (i.index)}
						<li>
							<button
								type="button"
								onclick={() => selectIndex(i)}
								class={'flex w-full items-start gap-2 border-b border-border/30 px-3 py-2 text-left transition-colors ' +
									(selected?.index === i.index ? 'bg-muted' : 'hover:bg-muted/50')}
							>
								<SearchIcon class="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
								<div class="min-w-0 flex-1">
									<div class="truncate font-mono text-sm">{i.index}</div>
									<div class="text-[10px] text-muted-foreground">
										{i.docsCount} doc{i.docsCount === '1' ? '' : 's'}
									</div>
								</div>
							</button>
						</li>
					{/each}
				</ul>
			{/if}
		</aside>

		<!-- Detail pane -->
		<section class="flex min-h-0 min-w-0 flex-col">
			{#if !selected}
				<div class="flex h-full items-center justify-center p-6 text-sm text-muted-foreground">
					Select an index to inspect.
				</div>
			{:else}
				<div
					class="flex shrink-0 items-center justify-between gap-2 border-b border-border bg-background/40 px-4 py-2"
				>
					<div class="flex items-center gap-2">
						<span class="text-xs font-medium text-muted-foreground">Index</span>
						<span class="font-mono text-sm">{selected.index}</span>
						<Badge variant="outline">{total} total</Badge>
					</div>
					<div class="flex items-center gap-1">
						<Button
							variant="ghost"
							size="sm"
							onclick={() => {
								putOpen = true;
								putId = '';
							}}
						>
							<Plus class="size-3.5" />
							New doc
						</Button>
						<Button
							variant="ghost"
							size="sm"
							onclick={() => handleDeleteIndex(selected!)}
						>
							<Trash2 class="size-3.5 text-destructive" />
							Delete index
						</Button>
					</div>
				</div>

				<Tabs bind:value={active} class="flex min-h-0 min-w-0 flex-1 flex-col gap-0">
					<TabsList class="mx-4 mt-2 self-start">
						<TabsTrigger value="browse">Browse</TabsTrigger>
						<TabsTrigger value="query">Query DSL</TabsTrigger>
						<TabsTrigger value="mapping">Mapping</TabsTrigger>
					</TabsList>

					<div class="min-h-0 min-w-0 flex-1 overflow-y-auto p-4">
						<TabsContent value="browse" class="m-0">
							{#if docsLoading}
								<div class="flex h-32 items-center justify-center text-muted-foreground">
									<Loader2 class="size-4 animate-spin" />
								</div>
							{:else if docs.length === 0}
								<p class="text-xs text-muted-foreground">No documents.</p>
							{:else}
								<ul class="space-y-2">
									{#each docs as h (h._id)}
										<li class="rounded border border-border/60 p-3">
											<div class="mb-1 flex items-center justify-between">
												<div class="flex items-center gap-2">
													<Badge variant="outline" class="font-mono text-[11px]">
														_id: {h._id}
													</Badge>
													{#if h._score !== null}
														<span class="text-[10px] text-muted-foreground">
															score {h._score.toFixed(2)}
														</span>
													{/if}
												</div>
												<Button
													variant="ghost"
													size="icon-sm"
													onclick={() => handleDeleteDoc(h._id)}
													aria-label="Delete document"
												>
													<Trash2 class="size-3.5" />
												</Button>
											</div>
											<pre class="overflow-x-auto rounded bg-muted/30 p-2 font-mono text-xs">{JSON.stringify(h._source, null, 2)}</pre>
										</li>
									{/each}
								</ul>
							{/if}
						</TabsContent>

						<TabsContent value="query" class="m-0">
							<div class="space-y-3">
								<div>
									<Label for="os-query">Query DSL</Label>
									<p class="mt-0.5 mb-1 text-xs text-muted-foreground">
										Standard OpenSearch query DSL. AWSim supports
										<code>match_all</code>, <code>match</code>,
										<code>term</code>, <code>terms</code>, <code>range</code>,
										<code>bool</code>, <code>wildcard</code>, <code>prefix</code>,
										<code>exists</code>, <code>ids</code>,
										<code>multi_match</code>, <code>query_string</code>,
										<code>knn</code> (no aggregations).
									</p>
									<Textarea
										id="os-query"
										bind:value={queryText}
										rows={10}
										class="font-mono text-xs"
									/>
								</div>
								<div class="flex justify-end">
									<Button onclick={runQuery} disabled={querying}>
										{#if querying}
											<Loader2 class="size-3.5 animate-spin" />
										{:else}
											<Play class="size-3.5" />
										{/if}
										Run
									</Button>
								</div>
								{#if queryResult.length > 0 || queryTotal > 0}
									<div>
										<div class="mb-2 text-xs text-muted-foreground">
											{queryTotal} total · {queryResult.length} returned
										</div>
										<ul class="space-y-2">
											{#each queryResult as h (h._id)}
												<li class="rounded border border-border/60 p-3">
													<div class="mb-1 flex items-center gap-2">
														<Badge variant="outline" class="font-mono text-[11px]">
															_id: {h._id}
														</Badge>
														{#if h._score !== null}
															<span class="text-[10px] text-muted-foreground">
																score {h._score.toFixed(2)}
															</span>
														{/if}
													</div>
													<pre class="overflow-x-auto rounded bg-muted/30 p-2 font-mono text-xs">{JSON.stringify(h._source, null, 2)}</pre>
												</li>
											{/each}
										</ul>
									</div>
								{/if}
							</div>
						</TabsContent>

						<TabsContent value="mapping" class="m-0">
							{#if mapping}
								<pre class="overflow-x-auto rounded bg-muted/30 p-3 font-mono text-xs">{JSON.stringify(mapping, null, 2)}</pre>
							{:else}
								<p class="text-xs text-muted-foreground">No mapping loaded.</p>
							{/if}
						</TabsContent>
					</div>
				</Tabs>
			{/if}
		</section>
	</div>
</ServicePage>

<Dialog bind:open={createOpen} onOpenChange={(v) => (createOpen = v)}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Create index</DialogTitle>
			<DialogDescription>
				The index is created on first use too — typing a name and indexing a document is
				equivalent. This dialog just lets you pre-create empty indices.
			</DialogDescription>
		</DialogHeader>
		<div class="py-2">
			<Label for="os-create-name">Name</Label>
			<Input
				id="os-create-name"
				bind:value={newIndexName}
				placeholder="my-index"
				class="font-mono"
			/>
		</div>
		<DialogFooter>
			<Button variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
			<Button onclick={handleCreate}>Create</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>

<Dialog bind:open={putOpen} onOpenChange={(v) => (putOpen = v)}>
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle>Index document</DialogTitle>
			<DialogDescription>
				PUTs a document at <code>/{selected?.index ?? ''}/_doc/&lt;id&gt;</code>. Replaces any
				existing document with the same ID.
			</DialogDescription>
		</DialogHeader>
		<div class="space-y-3 py-2">
			<div>
				<Label for="os-put-id">Document ID</Label>
				<Input id="os-put-id" bind:value={putId} placeholder="user-1" class="font-mono" />
			</div>
			<div>
				<Label for="os-put-body">JSON body</Label>
				<Textarea id="os-put-body" bind:value={putBody} rows={10} class="font-mono text-xs" />
			</div>
		</div>
		<DialogFooter>
			<Button variant="outline" onclick={() => (putOpen = false)}>Cancel</Button>
			<Button onclick={savePutDoc}>Save</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
