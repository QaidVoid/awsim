<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { onMount } from 'svelte';
	import { ServicePage, EmptyState, ListSkeleton } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import {
		Tabs,
		TabsList,
		TabsTrigger,
		TabsContent,
	} from '$lib/components/ui/tabs';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import GlobeIcon from '@lucide/svelte/icons/globe';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import InfoIcon from '@lucide/svelte/icons/info';
	import { toast } from 'svelte-sonner';
	import {
		getRestApis,
		createRestApi,
		deleteRestApi,
		type RestApi,
	} from '$lib/api/apigateway';
	import ApiList from '$lib/components/apigateway/api-list.svelte';
	import ResourcesTab from '$lib/components/apigateway/resources-tab.svelte';
	import StagesTab from '$lib/components/apigateway/stages-tab.svelte';
	import DeploymentsTab from '$lib/components/apigateway/deployments-tab.svelte';
	import AuthorizersTab from '$lib/components/apigateway/authorizers-tab.svelte';
	import RouteTester from '$lib/components/apigateway/route-tester.svelte';
	import ApiDetailSheet from '$lib/components/apigateway/api-detail-sheet.svelte';

	let apis = $state<RestApi[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	let selectedId = $state<string | null>(null);
	let active: string = $state(
		useTab('apigateway', ['resources', 'stages', 'deployments', 'authorizers', 'tester'] as const, 'resources', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);

	let createOpen = $state(false);
	let createName = $state('');
	let createDescription = $state('');
	let creating = $state(false);

	let detailOpen = $state(false);
	let confirmDelete = $state<RestApi | null>(null);

	let selected = $derived(apis.find((a) => a.id === selectedId) ?? null);

	async function load() {
		loading = true;
		error = null;
		try {
			apis = await getRestApis();
			if (selectedId && !apis.some((a) => a.id === selectedId)) selectedId = null;
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load APIs';
		} finally {
			loading = false;
		}
	}

	async function create(e: Event) {
		e.preventDefault();
		if (!createName.trim()) return;
		creating = true;
		try {
			const api = await createRestApi({
				name: createName.trim(),
				description: createDescription.trim() || undefined,
			});
			toast.success(`Created ${api.name}`);
			createName = '';
			createDescription = '';
			createOpen = false;
			selectedId = api.id;
			await load();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Create failed');
		} finally {
			creating = false;
		}
	}

	async function doDelete() {
		if (!confirmDelete) return;
		const api = confirmDelete;
		confirmDelete = null;
		try {
			await deleteRestApi(api.id);
			toast.success(`Deleted ${api.name}`);
			if (selectedId === api.id) selectedId = null;
			await load();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		}
	}

	onMount(load);
</script>

<ServicePage
	title="API Gateway"
	description="REST APIs, resources, stages, and deployments."
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={load} disabled={loading}>
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
				icon={GlobeIcon}
				title="No REST APIs"
				description="Create a REST API to start defining resources, methods, and stages."
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
		<div class="grid h-full min-h-0 grid-cols-[320px_1fr]">
			<ApiList
				{apis}
				{selectedId}
				{loading}
				onSelect={(a) => (selectedId = a.id)}
			/>

			<div class="flex h-full min-h-0 flex-col overflow-hidden">
				{#if selected}
					<header
						class="flex items-center justify-between gap-3 border-b border-border px-5 py-3"
					>
						<div class="min-w-0">
							<h2 class="truncate text-sm font-medium">{selected.name || selected.id}</h2>
							<p class="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">
								{selected.id}
							</p>
						</div>
						<div class="flex items-center gap-2">
							<Button
								size="sm"
								variant="outline"
								onclick={() => (detailOpen = true)}
							>
								<InfoIcon />
								Details
							</Button>
							<Button
								size="sm"
								variant="destructive"
								onclick={() => (confirmDelete = selected)}
							>
								<Trash2Icon />
								Delete
							</Button>
						</div>
					</header>

				<Tabs
					bind:value={active}
					class="flex h-full min-h-0 flex-1 flex-col overflow-hidden"
				>
						<TabsList variant="line" class="border-b border-border px-4">
							<TabsTrigger value="resources">Resources</TabsTrigger>
							<TabsTrigger value="stages">Stages</TabsTrigger>
							<TabsTrigger value="deployments">Deployments</TabsTrigger>
							<TabsTrigger value="authorizers">Authorizers</TabsTrigger>
							<TabsTrigger value="tester">Test</TabsTrigger>
						</TabsList>

						<div class="min-h-0 flex-1 overflow-y-auto">
							<TabsContent value="resources" class="m-0">
								<ResourcesTab restApiId={selected.id} />
							</TabsContent>
							<TabsContent value="stages" class="m-0">
								<StagesTab restApiId={selected.id} />
							</TabsContent>
							<TabsContent value="deployments" class="m-0 h-full">
								<DeploymentsTab restApiId={selected.id} />
							</TabsContent>
							<TabsContent value="authorizers" class="m-0 h-full">
								<AuthorizersTab restApiId={selected.id} />
							</TabsContent>
							<TabsContent value="tester" class="m-0 h-full">
								<RouteTester restApiId={selected.id} />
							</TabsContent>
						</div>
					</Tabs>
				{:else}
					<div class="flex h-full items-center justify-center text-sm text-muted-foreground">
						Select an API to inspect.
					</div>
				{/if}
			</div>
		</div>
	{/if}
</ServicePage>

<ApiDetailSheet
	open={detailOpen}
	api={selected}
	onOpenChange={(o) => (detailOpen = o)}
/>

<Dialog open={createOpen} onOpenChange={(o) => (createOpen = o)}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Create REST API</DialogTitle>
			<DialogDescription>Provision a new API Gateway REST API.</DialogDescription>
		</DialogHeader>
		<form onsubmit={create} class="grid gap-3 py-2">
			<div class="flex flex-col gap-1.5">
				<Label for="api-name">Name</Label>
				<Input id="api-name" bind:value={createName} placeholder="my-api" required />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="api-desc">Description</Label>
				<Input id="api-desc" bind:value={createDescription} placeholder="optional" />
			</div>
			<DialogFooter>
				<Button type="button" variant="ghost" onclick={() => (createOpen = false)}>
					Cancel
				</Button>
				<Button type="submit" disabled={creating || !createName.trim()}>
					<PlusIcon />
					{creating ? 'Creating...' : 'Create'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

<Dialog
	open={confirmDelete !== null}
	onOpenChange={(o) => {
		if (!o) confirmDelete = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Delete REST API?</DialogTitle>
			<DialogDescription>
				This permanently removes <span class="font-mono">{confirmDelete?.name}</span> and
				all of its resources, methods, and deployments.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmDelete = null)}>Cancel</Button>
			<Button variant="destructive" onclick={doDelete}>Delete</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
