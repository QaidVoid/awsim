<script lang="ts">
	/**
	 * AWS Batch page — tabs for compute environments, job queues, job
	 * definitions, and jobs. Submit + detail sheet for jobs.
	 */
	import { useTab } from '$lib/util/tab.svelte';
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import {
		ComputeEnvsTab,
		JobQueuesTab,
		JobDefinitionsTab,
		JobsTab,
		SubmitJobDialog,
		JobDetailSheet
	} from '$lib/components/batch';
	import type { JobSummary } from '$lib/api/batch';

	let active: string = $state(
		useTab('batch', ['envs', 'queues', 'defs', 'jobs'] as const, 'envs', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);

	let detailJob = $state<JobSummary | null>(null);
	let detailOpen = $state(false);
	let submitOpen = $state(false);
	let jobsRefresh = $state(0);

	function selectJob(j: JobSummary) {
		detailJob = j;
		detailOpen = true;
	}

	function bumpJobs() {
		jobsRefresh++;
	}
</script>

<svelte:head>
	<title>AWSim · Batch</title>
</svelte:head>

<ServicePage title="Batch" description="Run batch compute workloads with managed queues and jobs.">
	<Tabs bind:value={active} class="flex h-full min-h-0 flex-col">
		<TabsList class="mx-4 mt-2 self-start">
			<TabsTrigger value="envs">Compute environments</TabsTrigger>
			<TabsTrigger value="queues">Job queues</TabsTrigger>
			<TabsTrigger value="defs">Job definitions</TabsTrigger>
			<TabsTrigger value="jobs">Jobs</TabsTrigger>
		</TabsList>
		<div class="min-h-0 flex-1">
			<TabsContent value="envs" class="m-0 h-full">
				<ComputeEnvsTab />
			</TabsContent>
			<TabsContent value="queues" class="m-0 h-full">
				<JobQueuesTab />
			</TabsContent>
			<TabsContent value="defs" class="m-0 h-full">
				<JobDefinitionsTab />
			</TabsContent>
			<TabsContent value="jobs" class="m-0 h-full">
				<JobsTab onSelect={selectJob} onSubmit={() => (submitOpen = true)} refreshTick={jobsRefresh} />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<SubmitJobDialog
	open={submitOpen}
	onOpenChange={(o) => (submitOpen = o)}
	onSubmitted={bumpJobs}
/>
<JobDetailSheet job={detailJob} open={detailOpen} onOpenChange={(o) => (detailOpen = o)} />
