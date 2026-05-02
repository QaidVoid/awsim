<script lang="ts">
	/**
	 * CloudTrail page — Trails + Event History tabs.
	 */
	import { useTab } from '$lib/util/tab.svelte';
	import { ServicePage } from '$lib/components/service';
	import {
		Tabs,
		TabsContent,
		TabsList,
		TabsTrigger,
	} from '$lib/components/ui/tabs';
	import TrailsTab from '$lib/components/cloudtrail/trails-tab.svelte';
	import EventHistoryTab from '$lib/components/cloudtrail/event-history-tab.svelte';

	let active: string = $state(
		useTab('cloudtrail', ['trails', 'events'] as const, 'trails', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);
</script>

<svelte:head>
	<title>AWSim · CloudTrail</title>
</svelte:head>

<ServicePage title="CloudTrail" description="Audit trails for AWS API activity.">
	<Tabs
		bind:value={active}
		class="flex h-full min-h-0 flex-col"
	>
		<div class="border-b border-border px-4 pt-3">
			<TabsList>
				<TabsTrigger value="trails">Trails</TabsTrigger>
				<TabsTrigger value="events">Event history</TabsTrigger>
			</TabsList>
		</div>
		<TabsContent value="trails" class="min-h-0 flex-1">
			<TrailsTab />
		</TabsContent>
		<TabsContent value="events" class="min-h-0 flex-1">
			<EventHistoryTab />
		</TabsContent>
	</Tabs>
</ServicePage>
