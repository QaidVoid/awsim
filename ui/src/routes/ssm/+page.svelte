<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import ParametersTab from '$lib/components/ssm/parameters-tab.svelte';
	import DocumentsTab from '$lib/components/ssm/documents-tab.svelte';
	import ActivationsTab from '$lib/components/ssm/activations-tab.svelte';

	let activeTab = $state<'parameters' | 'documents' | 'activations'>('parameters');
</script>

<ServicePage
	title="Systems Manager"
	description="Parameters, automation documents, and hybrid activations."
>
	<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="parameters">Parameters</TabsTrigger>
			<TabsTrigger value="documents">Documents</TabsTrigger>
			<TabsTrigger value="activations">Activations</TabsTrigger>
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
		</div>
	</Tabs>
</ServicePage>
