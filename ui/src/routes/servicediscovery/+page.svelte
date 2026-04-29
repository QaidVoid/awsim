<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import NamespacesTab from '$lib/components/servicediscovery/namespaces-tab.svelte';
	import ServicesTab from '$lib/components/servicediscovery/services-tab.svelte';
	import InstancesSheet from '$lib/components/servicediscovery/instances-sheet.svelte';
	import type { SDService } from '$lib/api/servicediscovery';

	let activeTab = $state<'namespaces' | 'services'>('namespaces');
	let detailOpen = $state(false);
	let detailService = $state<SDService | null>(null);
	let refreshKey = $state(0);

	function refresh() {
		refreshKey += 1;
	}

	function openInstances(svc: SDService) {
		detailService = svc;
		detailOpen = true;
	}
</script>

<ServicePage
	title="Cloud Map"
	description="Service discovery — namespaces, services, and instances. Used by ECS service discovery."
>
	<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="namespaces">Namespaces</TabsTrigger>
			<TabsTrigger value="services">Services</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="namespaces" class="m-0">
				<NamespacesTab {refreshKey} onChanged={refresh} />
			</TabsContent>
			<TabsContent value="services" class="m-0">
				<ServicesTab {refreshKey} onSelect={openInstances} onChanged={refresh} />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<InstancesSheet
	open={detailOpen}
	service={detailService}
	onOpenChange={(o) => (detailOpen = o)}
	onChanged={refresh}
/>
