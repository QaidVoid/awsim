<script lang="ts">
	import { onMount } from 'svelte';
	import { ServicePage } from '$lib/components/service';
	import {
		Tabs,
		TabsList,
		TabsTrigger,
		TabsContent,
	} from '$lib/components/ui/tabs';
	import { Badge } from '$lib/components/ui/badge';
	import FoundationModelsTab from '$lib/components/bedrock/foundation-models-tab.svelte';
	import GuardrailsTab from '$lib/components/bedrock/guardrails-tab.svelte';
	import ProvisionedTab from '$lib/components/bedrock/provisioned-tab.svelte';
	import CustomModelsTab from '$lib/components/bedrock/custom-models-tab.svelte';
	import KnowledgeBasesTab from '$lib/components/bedrock/knowledge-bases-tab.svelte';
	import InvokePlayground from '$lib/components/bedrock/invoke-playground.svelte';
	import ProxyConfigTab from '$lib/components/bedrock/proxy-config-tab.svelte';
	import ModelDetailSheet from '$lib/components/bedrock/model-detail-sheet.svelte';
	import GuardrailDetailSheet from '$lib/components/bedrock/guardrail-detail-sheet.svelte';
	import CreateGuardrailDialog from '$lib/components/bedrock/create-guardrail-dialog.svelte';
	import {
		getBedrockProxyConfig,
		type FoundationModel,
		type Guardrail,
	} from '$lib/api/bedrock';

	type TabKey =
		| 'models'
		| 'guardrails'
		| 'provisioned'
		| 'custom'
		| 'knowledge'
		| 'playground'
		| 'proxy';

	let activeTab = $state<TabKey>('models');

	let modelSheetOpen = $state(false);
	let selectedModel = $state<FoundationModel | null>(null);

	let guardrailSheetOpen = $state(false);
	let selectedGuardrailId = $state<string | null>(null);

	let createGuardrailOpen = $state(false);
	let guardrailsReload = $state(0);

	let proxyEnabled = $state<boolean | null>(null);
	let proxyBackendCount = $state(0);

	onMount(async () => {
		try {
			const cfg = await getBedrockProxyConfig();
			proxyEnabled = cfg.enabled;
			proxyBackendCount = cfg.backends.length;
		} catch {
			proxyEnabled = false;
		}
	});

	function handleSelectModel(m: FoundationModel) {
		selectedModel = m;
		modelSheetOpen = true;
	}

	function handleSelectGuardrail(g: Guardrail) {
		selectedGuardrailId = g.guardrailId;
		guardrailSheetOpen = true;
	}

	const noop = () => {};
</script>

{#snippet proxyChip()}
	{#if proxyEnabled === null}
		<Badge variant="outline" class="text-xs">proxy: …</Badge>
	{:else if proxyEnabled}
		<button
			type="button"
			class="cursor-pointer"
			onclick={() => (activeTab = 'proxy')}
			title="View proxy config"
		>
			<Badge variant="default" class="text-xs">
				proxy: {proxyBackendCount} backend{proxyBackendCount === 1 ? '' : 's'}
			</Badge>
		</button>
	{:else}
		<button
			type="button"
			class="cursor-pointer"
			onclick={() => (activeTab = 'proxy')}
			title="No backend configured — using canned responses"
		>
			<Badge variant="outline" class="text-xs">proxy: canned</Badge>
		</button>
	{/if}
{/snippet}

<ServicePage
	title="Bedrock"
	description="Foundation models, guardrails, custom models, knowledge bases, and an invoke playground."
	actions={proxyChip}
>
	<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="models">Foundation models</TabsTrigger>
			<TabsTrigger value="guardrails">Guardrails</TabsTrigger>
			<TabsTrigger value="provisioned">Provisioned</TabsTrigger>
			<TabsTrigger value="custom">Custom models</TabsTrigger>
			<TabsTrigger value="knowledge">Knowledge bases</TabsTrigger>
			<TabsTrigger value="playground">Playground</TabsTrigger>
			<TabsTrigger value="proxy">Proxy config</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="models" class="m-0">
				<FoundationModelsTab onSelect={handleSelectModel} />
			</TabsContent>
			<TabsContent value="guardrails" class="m-0">
				{#key guardrailsReload}
					<GuardrailsTab
						onCreate={() => (createGuardrailOpen = true)}
						onSelect={handleSelectGuardrail}
					/>
				{/key}
			</TabsContent>
			<TabsContent value="provisioned" class="m-0">
				<ProvisionedTab />
			</TabsContent>
			<TabsContent value="custom" class="m-0">
				<CustomModelsTab onSelect={noop} />
			</TabsContent>
			<TabsContent value="knowledge" class="m-0">
				<KnowledgeBasesTab onSelect={noop} />
			</TabsContent>
			<TabsContent value="playground" class="m-0 h-full">
				<InvokePlayground />
			</TabsContent>
			<TabsContent value="proxy" class="m-0">
				<ProxyConfigTab />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<ModelDetailSheet
	open={modelSheetOpen}
	model={selectedModel}
	onOpenChange={(o) => (modelSheetOpen = o)}
/>

<GuardrailDetailSheet
	open={guardrailSheetOpen}
	guardrailId={selectedGuardrailId}
	onOpenChange={(o) => (guardrailSheetOpen = o)}
/>

<CreateGuardrailDialog
	open={createGuardrailOpen}
	onOpenChange={(o) => (createGuardrailOpen = o)}
	onCreated={() => (guardrailsReload += 1)}
/>
