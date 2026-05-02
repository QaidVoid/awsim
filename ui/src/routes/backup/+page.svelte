<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import VaultsTab from '$lib/components/backup/vaults-tab.svelte';
	import PlansTab from '$lib/components/backup/plans-tab.svelte';
	import JobsTab from '$lib/components/backup/jobs-tab.svelte';

	let active: string = $state(
		useTab('backup', ['vaults', 'plans', 'jobs'] as const, 'vaults', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);
	let refreshKey = $state(0);

	function refresh() {
		refreshKey += 1;
	}
</script>

<ServicePage
	title="Backup"
	description="Backup vaults, plans, and jobs across EFS, RDS, DynamoDB, S3, and EBS."
>
	<Tabs bind:value={active} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="vaults">Vaults</TabsTrigger>
			<TabsTrigger value="plans">Plans</TabsTrigger>
			<TabsTrigger value="jobs">Jobs</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="vaults" class="m-0">
				<VaultsTab {refreshKey} onChanged={refresh} />
			</TabsContent>
			<TabsContent value="plans" class="m-0">
				<PlansTab {refreshKey} onChanged={refresh} />
			</TabsContent>
			<TabsContent value="jobs" class="m-0">
				<JobsTab {refreshKey} onChanged={refresh} />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>
