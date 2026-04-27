<script lang="ts">
	import { onMount } from 'svelte';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import SchedulesTab from '$lib/components/scheduler/schedules-tab.svelte';
	import ScheduleGroupsTab from '$lib/components/scheduler/schedule-groups-tab.svelte';
	import ScheduleDetailSheet from '$lib/components/scheduler/schedule-detail-sheet.svelte';
	import CreateScheduleDialog from '$lib/components/scheduler/create-schedule-dialog.svelte';
	import { listScheduleGroups, type ScheduleGroup, type ScheduleSummary } from '$lib/api/scheduler';
	import { toast } from 'svelte-sonner';

	let activeTab = $state<'schedules' | 'groups'>('schedules');
	let selectedGroup = $state('ALL');
	let groups = $state<ScheduleGroup[]>([]);
	let refreshKey = $state(0);

	let createOpen = $state(false);

	let detailOpen = $state(false);
	let detailName = $state<string | null>(null);
	let detailGroup = $state('default');

	onMount(loadGroups);

	async function loadGroups() {
		try {
			groups = await listScheduleGroups();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load groups');
		}
	}

	function openDetail(s: ScheduleSummary) {
		detailName = s.name;
		detailGroup = s.groupName;
		detailOpen = true;
	}

	function refresh() {
		refreshKey += 1;
	}
</script>

<ServicePage
	title="EventBridge Scheduler"
	description="Schedule one-time or recurring tasks at any scale across AWS targets."
>
	{#snippet actions()}
		<Button size="sm" onclick={() => (createOpen = true)}>
			<PlusIcon />
			New schedule
		</Button>
	{/snippet}

	<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="schedules">Schedules</TabsTrigger>
			<TabsTrigger value="groups">Groups</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="schedules" class="m-0">
				<div class="flex items-center gap-2 border-b border-border px-4 py-2">
					<label for="sched-group-filter" class="text-xs text-muted-foreground">
						Group
					</label>
					<select
						id="sched-group-filter"
						bind:value={selectedGroup}
						class="h-7 rounded-md border border-border bg-background px-2 text-xs"
					>
						<option value="ALL">All groups</option>
						{#each groups as g (g.arn)}
							<option value={g.name}>{g.name}</option>
						{/each}
					</select>
				</div>
				<SchedulesTab
					groupName={selectedGroup}
					onSelect={openDetail}
					onCreate={() => (createOpen = true)}
					{refreshKey}
				/>
			</TabsContent>
			<TabsContent value="groups" class="m-0">
				<ScheduleGroupsTab
					onChange={() => {
						loadGroups();
						refresh();
					}}
				/>
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<CreateScheduleDialog
	open={createOpen}
	{groups}
	defaultGroup={selectedGroup === 'ALL' ? 'default' : selectedGroup}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={() => {
		refresh();
		loadGroups();
	}}
/>

<ScheduleDetailSheet
	open={detailOpen}
	name={detailName}
	groupName={detailGroup}
	onOpenChange={(o) => (detailOpen = o)}
	onDeleted={refresh}
/>
