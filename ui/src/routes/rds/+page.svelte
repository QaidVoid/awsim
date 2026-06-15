<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		describeDBInstances,
		deleteDBInstance,
		describeDBClusters,
		deleteDBCluster,
		type DBInstance,
		type DBCluster
	} from '$lib/api/rds';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import InstanceList from '$lib/components/rds/instance-list.svelte';
	import InstanceDetailSheet from '$lib/components/rds/instance-detail-sheet.svelte';
	import ClusterList from '$lib/components/rds/cluster-list.svelte';
	import ClusterDetailSheet from '$lib/components/rds/cluster-detail-sheet.svelte';
	import SnapshotsTab from '$lib/components/rds/snapshots-tab.svelte';
	import CreateInstanceDialog from '$lib/components/rds/create-instance-dialog.svelte';
	import CreateClusterDialog from '$lib/components/rds/create-cluster-dialog.svelte';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import Plus from '@lucide/svelte/icons/plus';

	let instances = $state<DBInstance[]>([]);
	let clusters = $state<DBCluster[]>([]);
	let loading = $state(true);
	let clustersLoading = $state(true);

	let active: string = $state(
		useTab('rds', ['instances', 'clusters', 'snapshots'] as const, 'instances', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);

	let selected = $state<DBInstance | null>(null);
	let detailOpen = $state(false);

	let selectedCluster = $state<DBCluster | null>(null);
	let clusterDetailOpen = $state(false);

	let createOpen = $state(false);
	let createClusterOpen = $state(false);
	let confirmOpen = $state(false);
	let confirmBusy = $state(false);
	let pendingDelete = $state<DBInstance | null>(null);
	let clusterConfirmOpen = $state(false);
	let clusterConfirmBusy = $state(false);
	let pendingClusterDelete = $state<DBCluster | null>(null);

	onMount(() => {
		void refresh();
		void refreshClusters();
	});

	async function refresh() {
		loading = true;
		try {
			instances = await describeDBInstances();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to list DB instances');
		} finally {
			loading = false;
		}
	}

	async function refreshClusters() {
		clustersLoading = true;
		try {
			clusters = await describeDBClusters();
			if (selectedCluster) {
				selectedCluster =
					clusters.find((c) => c.identifier === selectedCluster?.identifier) ?? null;
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to list clusters');
		} finally {
			clustersLoading = false;
		}
	}

	function openInstance(inst: DBInstance) {
		selected = inst;
		detailOpen = true;
	}

	function openCluster(cluster: DBCluster) {
		selectedCluster = cluster;
		clusterDetailOpen = true;
	}

	function askDelete(inst: DBInstance) {
		pendingDelete = inst;
		confirmOpen = true;
	}

	function askDeleteCluster(cluster: DBCluster) {
		pendingClusterDelete = cluster;
		clusterConfirmOpen = true;
	}

	async function confirmDelete() {
		if (!pendingDelete) return;
		confirmBusy = true;
		try {
			await deleteDBInstance(pendingDelete.identifier);
			toast.success(`Delete requested for ${pendingDelete.identifier}`);
			confirmOpen = false;
			detailOpen = false;
			pendingDelete = null;
			await refresh();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete instance');
		} finally {
			confirmBusy = false;
		}
	}

	async function confirmDeleteCluster() {
		if (!pendingClusterDelete) return;
		clusterConfirmBusy = true;
		try {
			await deleteDBCluster(pendingClusterDelete.identifier);
			toast.success(`Delete requested for ${pendingClusterDelete.identifier}`);
			clusterConfirmOpen = false;
			clusterDetailOpen = false;
			pendingClusterDelete = null;
			await refreshClusters();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete cluster');
		} finally {
			clusterConfirmBusy = false;
		}
	}

	async function onCreated() {
		await refresh();
	}

	async function onClusterCreated() {
		await refreshClusters();
	}
</script>

<ServicePage
	title="RDS"
	description="Relational Database Service: instances, Aurora clusters, and snapshots."
>
	{#snippet actions()}
		{#if active === 'clusters'}
			<Badge variant="outline" class="font-mono">
				{clusters.length} cluster{clusters.length === 1 ? '' : 's'}
			</Badge>
			<Button size="sm" onclick={() => (createClusterOpen = true)}>
				<Plus class="size-3.5" />
				Create cluster
			</Button>
		{:else}
			<Badge variant="outline" class="font-mono">
				{instances.length} instance{instances.length === 1 ? '' : 's'}
			</Badge>
			<Button size="sm" onclick={() => (createOpen = true)}>
				<Plus class="size-3.5" />
				Create instance
			</Button>
		{/if}
	{/snippet}

	<Tabs bind:value={active} class="flex h-full min-h-0 flex-col gap-0">
		<TabsList class="mx-4 mt-2 self-start">
			<TabsTrigger value="instances">Instances</TabsTrigger>
			<TabsTrigger value="clusters">Clusters</TabsTrigger>
			<TabsTrigger value="snapshots">Snapshots</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1">
			<TabsContent value="instances" class="m-0 h-full">
				<InstanceList
					{instances}
					{loading}
					selectedId={selected?.identifier ?? null}
					onSelect={openInstance}
					onRefresh={refresh}
				/>
			</TabsContent>
			<TabsContent value="clusters" class="m-0 h-full">
				<ClusterList
					{clusters}
					loading={clustersLoading}
					selectedId={selectedCluster?.identifier ?? null}
					onSelect={openCluster}
					onRefresh={refreshClusters}
				/>
			</TabsContent>
			<TabsContent value="snapshots" class="m-0 h-full">
				<SnapshotsTab />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<InstanceDetailSheet
	bind:open={detailOpen}
	instance={selected}
	onClose={() => (detailOpen = false)}
	onDeleteInstance={askDelete}
/>

<CreateInstanceDialog
	bind:open={createOpen}
	onClose={() => (createOpen = false)}
	onCreated={onCreated}
/>

<ConfirmDialog
	bind:open={confirmOpen}
	title="Delete DB instance?"
	description={`Permanently delete "${pendingDelete?.identifier ?? ''}". The final snapshot will be skipped.`}
	busy={confirmBusy}
	onConfirm={confirmDelete}
	onClose={() => (confirmOpen = false)}
/>

<ClusterDetailSheet
	bind:open={clusterDetailOpen}
	cluster={selectedCluster}
	onClose={() => (clusterDetailOpen = false)}
	onDeleteCluster={askDeleteCluster}
	onChanged={refreshClusters}
/>

<CreateClusterDialog
	bind:open={createClusterOpen}
	onClose={() => (createClusterOpen = false)}
	onCreated={onClusterCreated}
/>

<ConfirmDialog
	bind:open={clusterConfirmOpen}
	title="Delete DB cluster?"
	description={`Permanently delete "${pendingClusterDelete?.identifier ?? ''}". Delete its instances first if any remain.`}
	busy={clusterConfirmBusy}
	onConfirm={confirmDeleteCluster}
	onClose={() => (clusterConfirmOpen = false)}
/>
