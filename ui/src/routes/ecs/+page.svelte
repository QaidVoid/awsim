<script lang="ts">
	import { onMount } from 'svelte';
	import { listClusters, type Cluster, type Task } from '$lib/api/ecs';
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import { Badge } from '$lib/components/ui/badge';
	import {
		ClustersTab,
		ServicesTab,
		TasksTab,
		TaskDefinitionsTab,
		TaskDetailSheet
	} from '$lib/components/ecs';
	import { toast } from 'svelte-sonner';

	let clusters = $state<Cluster[]>([]);
	let loadingClusters = $state(true);
	let selectedCluster = $state<Cluster | null>(null);
	let activeTab = $state<'clusters' | 'services' | 'tasks' | 'taskdefs'>('clusters');

	let detailTask = $state<Task | null>(null);
	let detailOpen = $state(false);

	onMount(loadClusters);

	async function loadClusters() {
		loadingClusters = true;
		try {
			const r = await listClusters();
			clusters = r.clusters;
			if (selectedCluster) {
				selectedCluster = clusters.find((c) => c.arn === selectedCluster?.arn) ?? null;
			}
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load clusters');
		} finally {
			loadingClusters = false;
		}
	}

	function pickCluster(c: Cluster) {
		selectedCluster = c;
		if (activeTab === 'clusters') activeTab = 'services';
	}

	function openTask(t: Task) {
		detailTask = t;
		detailOpen = true;
	}
</script>

<ServicePage
	title="ECS"
	description="Run containers across clusters with tasks, services, and task definitions."
>
	{#snippet actions()}
		{#if selectedCluster}
			<Badge variant="outline" class="font-mono">{selectedCluster.name}</Badge>
		{/if}
	{/snippet}

	<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-col">
		<TabsList class="mx-4 mt-2 self-start">
			<TabsTrigger value="clusters">Clusters</TabsTrigger>
			<TabsTrigger value="services">Services</TabsTrigger>
			<TabsTrigger value="tasks">Tasks</TabsTrigger>
			<TabsTrigger value="taskdefs">Task definitions</TabsTrigger>
		</TabsList>
		<div class="min-h-0 flex-1">
			<TabsContent value="clusters" class="m-0 h-full">
				<ClustersTab
					{clusters}
					loading={loadingClusters}
					selectedArn={selectedCluster?.arn ?? null}
					onReload={loadClusters}
					onSelect={pickCluster}
				/>
			</TabsContent>
			<TabsContent value="services" class="m-0 h-full">
				<ServicesTab cluster={selectedCluster} />
			</TabsContent>
			<TabsContent value="tasks" class="m-0 h-full">
				<TasksTab cluster={selectedCluster} onSelect={openTask} />
			</TabsContent>
			<TabsContent value="taskdefs" class="m-0 h-full">
				<TaskDefinitionsTab />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<TaskDetailSheet task={detailTask} open={detailOpen} onOpenChange={(o) => (detailOpen = o)} />
