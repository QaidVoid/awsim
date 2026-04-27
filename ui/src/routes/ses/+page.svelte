<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import IdentitiesTab from '$lib/components/ses/identities-tab.svelte';
	import ConfigurationSetsTab from '$lib/components/ses/configuration-sets-tab.svelte';
	import TemplatesTab from '$lib/components/ses/templates-tab.svelte';
	import ContactListsTab from '$lib/components/ses/contact-lists-tab.svelte';
	import SuppressionListTab from '$lib/components/ses/suppression-list-tab.svelte';

	type TabId = 'identities' | 'config-sets' | 'templates' | 'contacts' | 'suppression';
	let activeTab = $state<TabId>('identities');
</script>

<ServicePage title="SES" description="Simple Email Service: identities, templates, contacts, and suppressions.">
	<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="identities">Identities</TabsTrigger>
			<TabsTrigger value="config-sets">Configuration sets</TabsTrigger>
			<TabsTrigger value="templates">Templates</TabsTrigger>
			<TabsTrigger value="contacts">Contact lists</TabsTrigger>
			<TabsTrigger value="suppression">Suppression list</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="identities" class="m-0">
				<IdentitiesTab />
			</TabsContent>
			<TabsContent value="config-sets" class="m-0">
				<ConfigurationSetsTab />
			</TabsContent>
			<TabsContent value="templates" class="m-0">
				<TemplatesTab />
			</TabsContent>
			<TabsContent value="contacts" class="m-0">
				<ContactListsTab />
			</TabsContent>
			<TabsContent value="suppression" class="m-0">
				<SuppressionListTab />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>
