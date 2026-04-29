<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import BrokersList from '$lib/components/mq/brokers-list.svelte';
	import CreateBrokerDialog from '$lib/components/mq/create-broker-dialog.svelte';
	import BrokerDetailSheet from '$lib/components/mq/broker-detail-sheet.svelte';
	import type { BrokerSummary } from '$lib/api/mq';

	let createOpen = $state(false);
	let detailOpen = $state(false);
	let detailSummary = $state<BrokerSummary | null>(null);
	let refreshKey = $state(0);

	function refresh() {
		refreshKey += 1;
	}

	function openDetail(b: BrokerSummary) {
		detailSummary = b;
		detailOpen = true;
	}
</script>

<ServicePage title="MQ" description="Amazon MQ brokers — ActiveMQ and RabbitMQ.">
	{#snippet actions()}
		<Button size="sm" onclick={() => (createOpen = true)}>
			<PlusIcon />
			Create broker
		</Button>
	{/snippet}

	<BrokersList onSelect={openDetail} onCreate={() => (createOpen = true)} {refreshKey} />
</ServicePage>

<CreateBrokerDialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={refresh}
/>

<BrokerDetailSheet
	open={detailOpen}
	summary={detailSummary}
	onOpenChange={(o) => (detailOpen = o)}
	onChanged={refresh}
/>
