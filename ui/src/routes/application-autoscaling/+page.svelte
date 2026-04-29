<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import TargetsList from '$lib/components/application-autoscaling/targets-list.svelte';
	import RegisterTargetDialog from '$lib/components/application-autoscaling/register-target-dialog.svelte';
	import TargetDetailSheet from '$lib/components/application-autoscaling/target-detail-sheet.svelte';
	import type { ScalableTarget } from '$lib/api/application-autoscaling';

	let serviceNamespace = $state('ecs');
	let createOpen = $state(false);
	let detailOpen = $state(false);
	let detailTarget = $state<ScalableTarget | null>(null);
	let refreshKey = $state(0);

	function refresh() {
		refreshKey += 1;
	}

	function openDetail(t: ScalableTarget) {
		detailTarget = t;
		detailOpen = true;
	}
</script>

<ServicePage
	title="Application Auto Scaling"
	description="Scalable targets and policies for ECS, Lambda, DynamoDB, AppStream, and more."
>
	{#snippet actions()}
		<Button size="sm" onclick={() => (createOpen = true)}>
			<PlusIcon />
			Register target
		</Button>
	{/snippet}

	<TargetsList
		bind:serviceNamespace
		onCreate={() => (createOpen = true)}
		onSelect={openDetail}
		{refreshKey}
	/>
</ServicePage>

<RegisterTargetDialog
	open={createOpen}
	defaultNamespace={serviceNamespace}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={refresh}
/>

<TargetDetailSheet
	open={detailOpen}
	target={detailTarget}
	onOpenChange={(o) => (detailOpen = o)}
	onChanged={refresh}
/>
