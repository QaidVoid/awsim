<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { onMount } from 'svelte';
	import { listClustersWithDetail, type Cluster } from '$lib/api/eks';
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import { Badge } from '$lib/components/ui/badge';
	import {
		ClusterList,
		NodegroupList,
		FargateProfileList
	} from '$lib/components/eks';
	import { toast } from 'svelte-sonner';

	let clusters = $state<Cluster[]>([]);
	let loading = $state(true);
	let selected = $state<Cluster | null>(null);
	let active: string = $state(
		useTab('eks', ['clusters', 'nodegroups', 'fargate'] as const, 'clusters', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);

	onMount(loadClusters);

	async function loadClusters() {
		loading = true;
		try {
			const r = await listClustersWithDetail();
			clusters = r.clusters;
			if (selected) {
				selected = clusters.find((c) => c.name === selected?.name) ?? null;
			}
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load clusters');
		} finally {
			loading = false;
		}
	}

	function pickCluster(c: Cluster) {
		selected = c;
		if (active === 'clusters') active = 'nodegroups';
	}
</script>

<ServicePage
	title="EKS"
	description="Managed Kubernetes — clusters, nodegroups, and Fargate profiles."
>
	{#snippet actions()}
		{#if selected}
			<Badge variant="outline" class="font-mono">{selected.name}</Badge>
		{/if}
	{/snippet}

	<Tabs bind:value={active} class="flex h-full min-h-0 flex-col">
		<TabsList class="mx-4 mt-2 self-start">
			<TabsTrigger value="clusters">Clusters</TabsTrigger>
			<TabsTrigger value="nodegroups">Nodegroups</TabsTrigger>
			<TabsTrigger value="fargate">Fargate profiles</TabsTrigger>
		</TabsList>
		<div class="min-h-0 flex-1">
			<TabsContent value="clusters" class="m-0 h-full">
				<ClusterList
					{clusters}
					{loading}
					selectedName={selected?.name ?? null}
					onReload={loadClusters}
					onSelect={pickCluster}
				/>
			</TabsContent>
			<TabsContent value="nodegroups" class="m-0 h-full">
				<NodegroupList cluster={selected} />
			</TabsContent>
			<TabsContent value="fargate" class="m-0 h-full">
				<FargateProfileList cluster={selected} />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>
