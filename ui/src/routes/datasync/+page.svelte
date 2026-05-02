<script lang="ts">
	/**
	 * DataSync page — locations, tasks, and executions.
	 */
	import { useTab } from '$lib/util/tab.svelte';
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import {
		LocationsTab,
		TasksTab,
		ExecutionsTab,
		LocationDetailSheet,
		TaskDetailSheet,
		CreateLocationS3Dialog
	} from '$lib/components/datasync';
	import type { Location, Task } from '$lib/api/datasync';

	let active: string = $state(
		useTab('datasync', ['locations', 'tasks', 'executions'] as const, 'locations', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);

	let selectedLocation = $state<Location | null>(null);
	let locationOpen = $state(false);

	let selectedTask = $state<Task | null>(null);
	let taskOpen = $state(false);

	let createOpen = $state(false);
	let locationsRefresh = $state(0);

	function selectLocation(l: Location) {
		selectedLocation = l;
		locationOpen = true;
	}

	function selectTask(t: Task) {
		selectedTask = t;
		taskOpen = true;
	}

	function bumpLocations() {
		locationsRefresh++;
	}
</script>

<svelte:head>
	<title>AWSim · DataSync</title>
</svelte:head>

<ServicePage title="DataSync" description="Move data between AWS storage services and on-prem.">
	<Tabs bind:value={active} class="flex h-full min-h-0 flex-col">
		<TabsList class="mx-4 mt-2 self-start">
			<TabsTrigger value="locations">Locations</TabsTrigger>
			<TabsTrigger value="tasks">Tasks</TabsTrigger>
			<TabsTrigger value="executions">Executions</TabsTrigger>
		</TabsList>
		<div class="min-h-0 flex-1">
			<TabsContent value="locations" class="m-0 h-full">
				<LocationsTab
					onSelect={selectLocation}
					onCreate={() => (createOpen = true)}
					refreshTick={locationsRefresh}
				/>
			</TabsContent>
			<TabsContent value="tasks" class="m-0 h-full">
				<TasksTab onSelect={selectTask} />
			</TabsContent>
			<TabsContent value="executions" class="m-0 h-full">
				<ExecutionsTab />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<LocationDetailSheet
	location={selectedLocation}
	open={locationOpen}
	onOpenChange={(o) => (locationOpen = o)}
/>
<TaskDetailSheet task={selectedTask} open={taskOpen} onOpenChange={(o) => (taskOpen = o)} />
<CreateLocationS3Dialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={bumpLocations}
/>
