<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { ServicePage } from '$lib/components/service';
	import {
		Tabs,
		TabsList,
		TabsTrigger,
		TabsContent,
	} from '$lib/components/ui/tabs';
	import QueryEditorTab from '$lib/components/athena/query-editor-tab.svelte';
	import HistoryTab from '$lib/components/athena/history-tab.svelte';
	import WorkgroupsTab from '$lib/components/athena/workgroups-tab.svelte';
	import NamedQueriesTab from '$lib/components/athena/named-queries-tab.svelte';

	let active: string = $state(
		useTab('athena', ['editor', 'history', 'workgroups', 'named'] as const, 'editor', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);
</script>

<ServicePage
	title="Athena"
	description="Run interactive SQL against data in S3 using workgroups and saved named queries."
>
	<Tabs bind:value={active} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="editor">Query editor</TabsTrigger>
			<TabsTrigger value="history">History</TabsTrigger>
			<TabsTrigger value="workgroups">WorkGroups</TabsTrigger>
			<TabsTrigger value="named">Named queries</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="editor" class="m-0">
				<QueryEditorTab />
			</TabsContent>
			<TabsContent value="history" class="m-0">
				<HistoryTab />
			</TabsContent>
			<TabsContent value="workgroups" class="m-0">
				<WorkgroupsTab />
			</TabsContent>
			<TabsContent value="named" class="m-0">
				<NamedQueriesTab />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>
