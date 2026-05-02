<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import ParametersTab from '$lib/components/ssm/parameters-tab.svelte';
	import DocumentsTab from '$lib/components/ssm/documents-tab.svelte';
	import ActivationsTab from '$lib/components/ssm/activations-tab.svelte';
	import MaintenanceWindowsTab from '$lib/components/ssm/maintenance-windows-tab.svelte';
	import OpsItemsTab from '$lib/components/ssm/ops-items-tab.svelte';

	let active: string = $state(
		useTab('ssm', ['parameters', 'documents', 'activations', 'windows', 'ops'] as const, 'parameters', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);
</script>

<ServicePage
	title="Systems Manager"
	description="Parameters, automation documents, and hybrid activations."
>
	<Tabs bind:value={active} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="parameters">Parameters</TabsTrigger>
			<TabsTrigger value="documents">Documents</TabsTrigger>
			<TabsTrigger value="activations">Activations</TabsTrigger>
			<TabsTrigger value="windows">Maintenance windows</TabsTrigger>
			<TabsTrigger value="ops">OpsItems</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="parameters" class="m-0">
				<ParametersTab />
			</TabsContent>
			<TabsContent value="documents" class="m-0">
				<DocumentsTab />
			</TabsContent>
			<TabsContent value="activations" class="m-0">
				<ActivationsTab />
			</TabsContent>
			<TabsContent value="windows" class="m-0">
				<MaintenanceWindowsTab />
			</TabsContent>
			<TabsContent value="ops" class="m-0">
				<OpsItemsTab />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>
