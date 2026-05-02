<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import { Button } from '$lib/components/ui/button';
	import SendIcon from '@lucide/svelte/icons/send';
	import IdentitiesTab from '$lib/components/ses/identities-tab.svelte';
	import ConfigurationSetsTab from '$lib/components/ses/configuration-sets-tab.svelte';
	import TemplatesTab from '$lib/components/ses/templates-tab.svelte';
	import ContactListsTab from '$lib/components/ses/contact-lists-tab.svelte';
	import SuppressionListTab from '$lib/components/ses/suppression-list-tab.svelte';
	import OutboxTab from '$lib/components/ses/outbox-tab.svelte';
	import SendEmailDialog from '$lib/components/ses/send-email-dialog.svelte';

	let active: string = $state(
		useTab('ses', ['outbox', 'identities', 'config-sets', 'templates', 'contacts', 'suppression'] as const, 'outbox', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);
	let composeOpen = $state(false);
</script>

<ServicePage title="SES" description="Simple Email Service: identities, templates, contacts, and suppressions.">
	{#snippet actions()}
		<Button size="sm" onclick={() => (composeOpen = true)}>
			<SendIcon /> Compose
		</Button>
	{/snippet}
	<Tabs bind:value={active} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="outbox">Outbox</TabsTrigger>
			<TabsTrigger value="identities">Identities</TabsTrigger>
			<TabsTrigger value="config-sets">Configuration sets</TabsTrigger>
			<TabsTrigger value="templates">Templates</TabsTrigger>
			<TabsTrigger value="contacts">Contact lists</TabsTrigger>
			<TabsTrigger value="suppression">Suppression list</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-hidden">
			<TabsContent value="outbox" class="m-0 h-full data-[state=inactive]:hidden">
				<OutboxTab />
			</TabsContent>
			<TabsContent value="identities" class="m-0 overflow-y-auto data-[state=inactive]:hidden">
				<IdentitiesTab />
			</TabsContent>
			<TabsContent value="config-sets" class="m-0 overflow-y-auto data-[state=inactive]:hidden">
				<ConfigurationSetsTab />
			</TabsContent>
			<TabsContent value="templates" class="m-0 overflow-y-auto data-[state=inactive]:hidden">
				<TemplatesTab />
			</TabsContent>
			<TabsContent value="contacts" class="m-0 overflow-y-auto data-[state=inactive]:hidden">
				<ContactListsTab />
			</TabsContent>
			<TabsContent value="suppression" class="m-0 overflow-y-auto data-[state=inactive]:hidden">
				<SuppressionListTab />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<SendEmailDialog open={composeOpen} onOpenChange={(o) => (composeOpen = o)} />
