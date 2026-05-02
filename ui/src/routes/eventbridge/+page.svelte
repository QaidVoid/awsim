<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import SendIcon from '@lucide/svelte/icons/send';
	import BusesTab from '$lib/components/eventbridge/buses-tab.svelte';
	import RulesTab from '$lib/components/eventbridge/rules-tab.svelte';
	import ArchivesTab from '$lib/components/eventbridge/archives-tab.svelte';
	import SendEventDialog from '$lib/components/eventbridge/send-event-dialog.svelte';

	let active: string = $state(
		useTab('eventbridge', ['buses', 'rules', 'archives'] as const, 'buses', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);
	let selectedBus = $state('default');

	let sendOpen = $state(false);
	let sendBus = $state('default');

	function openSend(bus: string) {
		sendBus = bus;
		sendOpen = true;
	}

	function selectBus(bus: string) {
		selectedBus = bus;
	}
</script>

<ServicePage
	title="EventBridge"
	description="Serverless event bus for routing application events between services."
>
	{#snippet actions()}
		<Button size="sm" onclick={() => openSend(selectedBus)}>
			<SendIcon />
			Send event
		</Button>
	{/snippet}

	<Tabs bind:value={active} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="buses">Event buses</TabsTrigger>
			<TabsTrigger value="rules">Rules</TabsTrigger>
			<TabsTrigger value="archives">Archives</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="buses" class="m-0">
				<BusesTab {selectedBus} onSelect={selectBus} onSendEvent={openSend} />
			</TabsContent>
			<TabsContent value="rules" class="m-0">
				<RulesTab busName={selectedBus} />
			</TabsContent>
			<TabsContent value="archives" class="m-0">
				<ArchivesTab />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<SendEventDialog open={sendOpen} busName={sendBus} onOpenChange={(o) => (sendOpen = o)} />
