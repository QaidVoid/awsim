<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import PipesList from '$lib/components/pipes/pipes-list.svelte';
	import CreatePipeDialog from '$lib/components/pipes/create-pipe-dialog.svelte';
	import PipeDetailSheet from '$lib/components/pipes/pipe-detail-sheet.svelte';
	import type { PipeSummary } from '$lib/api/pipes';

	let createOpen = $state(false);
	let detailOpen = $state(false);
	let detailName = $state<string | null>(null);
	let refreshKey = $state(0);

	function refresh() {
		refreshKey += 1;
	}

	function openDetail(p: PipeSummary) {
		detailName = p.name;
		detailOpen = true;
	}
</script>

<ServicePage
	title="EventBridge Pipes"
	description="Point-to-point integrations from a source (SQS) to a target (Lambda, Step Functions, SQS, SNS) with optional filters and enrichment."
>
	{#snippet actions()}
		<Button size="sm" onclick={() => (createOpen = true)}>
			<PlusIcon />
			New pipe
		</Button>
	{/snippet}

	<PipesList onSelect={openDetail} onCreate={() => (createOpen = true)} {refreshKey} />
</ServicePage>

<CreatePipeDialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={refresh}
/>

<PipeDetailSheet
	open={detailOpen}
	name={detailName}
	onOpenChange={(o) => (detailOpen = o)}
	onChanged={refresh}
/>
