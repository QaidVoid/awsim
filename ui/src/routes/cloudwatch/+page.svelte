<script lang="ts">
	/**
	 * CloudWatch Logs explorer — three-pane layout:
	 *   Left: log groups
	 *   Middle: streams scoped to the selected group
	 *   Right: events for the selected stream (auto-tail + filter)
	 */
	import { onMount } from 'svelte';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import {
		describeLogGroups,
		describeLogStreams,
		type LogGroup,
		type LogStream,
	} from '$lib/api/cloudwatch-logs';
	import LogGroupsPane from '$lib/components/cloudwatch-logs/log-groups-pane.svelte';
	import LogStreamsPane from '$lib/components/cloudwatch-logs/log-streams-pane.svelte';
	import LogEventsViewer from '$lib/components/cloudwatch-logs/log-events-viewer.svelte';
	import InsightsQueryDialog from '$lib/components/cloudwatch-logs/insights-query-dialog.svelte';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import { toast } from 'svelte-sonner';

	let groups = $state<LogGroup[]>([]);
	let streams = $state<LogStream[]>([]);
	let groupsLoading = $state(false);
	let streamsLoading = $state(false);
	let selectedGroup = $state<string | null>(null);
	let selectedStream = $state<string | null>(null);
	let insightsOpen = $state(false);

	async function loadGroups() {
		groupsLoading = true;
		try {
			const data = await describeLogGroups();
			groups = data.logGroups;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load log groups');
		} finally {
			groupsLoading = false;
		}
	}

	async function loadStreams(group: string) {
		streamsLoading = true;
		try {
			const data = await describeLogStreams(group);
			streams = data.logStreams;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load streams');
		} finally {
			streamsLoading = false;
		}
	}

	function selectGroup(name: string) {
		if (selectedGroup === name) return;
		selectedGroup = name;
		selectedStream = null;
		streams = [];
		loadStreams(name);
	}

	function selectStream(name: string) {
		selectedStream = name;
	}

	onMount(loadGroups);
</script>

<svelte:head>
	<title>AWSim · CloudWatch Logs</title>
</svelte:head>

<ServicePage title="CloudWatch Logs" description="Browse log groups, streams and events.">
	{#snippet actions()}
		<Button
			size="sm"
			variant="outline"
			class="h-7 gap-1.5 px-2 text-xs"
			disabled={!selectedGroup}
			onclick={() => (insightsOpen = true)}
		>
			<Sparkles class="size-3.5" />
			Insights
		</Button>
		<Button size="sm" variant="outline" class="h-7 px-2 text-xs" onclick={loadGroups}>
			Refresh
		</Button>
	{/snippet}

	<div class="grid h-full min-h-0 grid-cols-[280px_320px_minmax(0,1fr)]">
		<LogGroupsPane
			{groups}
			selected={selectedGroup}
			loading={groupsLoading}
			onSelect={selectGroup}
			onRefresh={loadGroups}
		/>
		<LogStreamsPane
			group={selectedGroup}
			{streams}
			selected={selectedStream}
			loading={streamsLoading}
			onSelect={selectStream}
			onRefresh={async () => {
				if (selectedGroup) await loadStreams(selectedGroup);
			}}
		/>
		<LogEventsViewer group={selectedGroup} stream={selectedStream} />
	</div>
</ServicePage>

<InsightsQueryDialog
	open={insightsOpen}
	group={selectedGroup}
	onOpenChange={(o) => (insightsOpen = o)}
/>
