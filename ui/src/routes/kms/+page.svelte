<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import KeysTab from '$lib/components/kms/keys-tab.svelte';
	import AliasesTab from '$lib/components/kms/aliases-tab.svelte';
	import EncryptDecryptPlayground from '$lib/components/kms/encrypt-decrypt-playground.svelte';

	let active = $state('keys');
</script>

<ServicePage
	title="KMS"
	description="Customer-managed keys, aliases, key policies, and a quick encrypt/decrypt playground."
>
	<Tabs bind:value={active} class="flex h-full min-h-0 flex-col">
		<div class="border-b border-border px-6 pt-3">
			<TabsList variant="line">
				<TabsTrigger value="keys">Keys</TabsTrigger>
				<TabsTrigger value="aliases">Aliases</TabsTrigger>
				<TabsTrigger value="playground">Playground</TabsTrigger>
			</TabsList>
		</div>

		<TabsContent value="keys" class="min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden">
			<KeysTab />
		</TabsContent>
		<TabsContent
			value="aliases"
			class="min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
		>
			<AliasesTab />
		</TabsContent>
		<TabsContent
			value="playground"
			class="min-h-0 flex-1 overflow-y-auto data-[state=inactive]:hidden"
		>
			<EncryptDecryptPlayground />
		</TabsContent>
	</Tabs>
</ServicePage>
