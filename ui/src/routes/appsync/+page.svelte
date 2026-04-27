<script lang="ts">
	import { onMount } from 'svelte';
	import { ServicePage, EmptyState, ListSkeleton } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter
	} from '$lib/components/ui/dialog';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import LayersIcon from '@lucide/svelte/icons/layers';
	import { toast } from 'svelte-sonner';
	import { listGraphqlApis, deleteGraphqlApi, type GraphqlApi } from '$lib/api/appsync';
	import ApiList from '$lib/components/appsync/api-list.svelte';
	import ApiDetail from '$lib/components/appsync/api-detail.svelte';
	import CreateApiDialog from '$lib/components/appsync/create-api-dialog.svelte';

	let apis = $state<GraphqlApi[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let selectedId = $state<string | null>(null);
	let createOpen = $state(false);
	let confirmDelete = $state<{ apiId: string; name: string } | null>(null);

	let selectedApi = $derived(apis.find((a) => a.apiId === selectedId) ?? null);

	async function loadApis() {
		loading = true;
		error = null;
		try {
			apis = await listGraphqlApis();
			if (selectedId && !apis.some((a) => a.apiId === selectedId)) selectedId = null;
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load APIs';
		} finally {
			loading = false;
		}
	}

	async function handleDelete() {
		if (!confirmDelete) return;
		const { apiId, name } = confirmDelete;
		confirmDelete = null;
		try {
			await deleteGraphqlApi(apiId);
			toast.success(`API ${name} deleted.`);
			if (selectedId === apiId) selectedId = null;
			await loadApis();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}

	onMount(loadApis);
</script>

<ServicePage
	title="AppSync"
	description="Managed GraphQL APIs — data sources, resolvers, functions, schema."
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={loadApis} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<PlusIcon />
			New API
		</Button>
	{/snippet}

	{#if error}
		<div class="px-6 py-4 text-sm text-destructive">{error}</div>
	{:else if loading && apis.length === 0}
		<div class="px-6 py-6">
			<ListSkeleton rows={6} />
		</div>
	{:else if apis.length === 0}
		<div class="px-6 py-12">
			<EmptyState
				icon={LayersIcon}
				title="No GraphQL APIs"
				description="Create an API to define a schema, resolvers, and data sources."
			>
				{#snippet action()}
					<Button onclick={() => (createOpen = true)}>
						<PlusIcon />
						Create API
					</Button>
				{/snippet}
			</EmptyState>
		</div>
	{:else}
		<div class="grid h-full min-h-0 grid-cols-[300px_1fr]">
			<ApiList
				{apis}
				{selectedId}
				onSelect={(id) => (selectedId = id)}
				onCreate={() => (createOpen = true)}
			/>

			<div class="flex h-full min-h-0 flex-col overflow-hidden">
				{#if selectedApi}
					<ApiDetail
						api={selectedApi}
						onDelete={() =>
							(confirmDelete = { apiId: selectedApi!.apiId, name: selectedApi!.name })}
					/>
				{:else}
					<div class="flex h-full items-center justify-center text-sm text-muted-foreground">
						Select an API to inspect.
					</div>
				{/if}
			</div>
		</div>
	{/if}
</ServicePage>

<CreateApiDialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={loadApis}
/>

<Dialog
	open={confirmDelete !== null}
	onOpenChange={(o) => {
		if (!o) confirmDelete = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Delete API?</DialogTitle>
			<DialogDescription>
				This permanently removes <span class="font-mono">{confirmDelete?.name}</span> and all
				of its resolvers, data sources, and functions.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmDelete = null)}>Cancel</Button>
			<Button variant="destructive" onclick={handleDelete}>Delete</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
