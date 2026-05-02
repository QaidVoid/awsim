<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { onMount } from 'svelte';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import {
		Tabs,
		TabsList,
		TabsTrigger,
		TabsContent,
	} from '$lib/components/ui/tabs';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import { toast } from 'svelte-sonner';
	import { listDistributions, type Distribution } from '$lib/api/cloudfront';
	import DistributionsTab from '$lib/components/cloudfront/distributions-tab.svelte';
	import OaiTab from '$lib/components/cloudfront/oai-tab.svelte';
	import CachePoliciesTab from '$lib/components/cloudfront/cache-policies-tab.svelte';
	import OriginRequestPoliciesTab from '$lib/components/cloudfront/origin-request-policies-tab.svelte';
	import KeyGroupsTab from '$lib/components/cloudfront/key-groups-tab.svelte';
	import PublicKeysTab from '$lib/components/cloudfront/public-keys-tab.svelte';
	import FunctionsTab from '$lib/components/cloudfront/functions-tab.svelte';
	import DistributionDetailSheet from '$lib/components/cloudfront/distribution-detail-sheet.svelte';
	import CreateDistributionDialog from '$lib/components/cloudfront/create-distribution-dialog.svelte';

	let active: string = $state(
		useTab('cloudfront', ['distributions', 'cache', 'origin', 'oai', 'keyGroups', 'publicKeys', 'functions'] as const, 'distributions', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);

	let distributions = $state<Distribution[]>([]);
	let distLoading = $state(false);

	let createOpen = $state(false);
	let detailOpen = $state(false);
	let selected = $state<Distribution | null>(null);

	async function loadDistributions() {
		distLoading = true;
		try {
			distributions = await listDistributions();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load distributions');
		} finally {
			distLoading = false;
		}
	}

	function openDetail(d: Distribution) {
		selected = d;
		detailOpen = true;
	}

	onMount(loadDistributions);
</script>

<ServicePage
	title="CloudFront"
	description="Edge-cached content delivery: distributions, policies, key management, and edge functions."
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={loadDistributions} disabled={distLoading}>
			<RefreshCwIcon class={distLoading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	{/snippet}

	<Tabs bind:value={active} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="distributions">Distributions</TabsTrigger>
			<TabsTrigger value="cache">Cache policies</TabsTrigger>
			<TabsTrigger value="origin">Origin request</TabsTrigger>
			<TabsTrigger value="oai">Origin access</TabsTrigger>
			<TabsTrigger value="keyGroups">Key groups</TabsTrigger>
			<TabsTrigger value="publicKeys">Public keys</TabsTrigger>
			<TabsTrigger value="functions">Functions</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="distributions" class="m-0">
				<DistributionsTab
					{distributions}
					loading={distLoading}
					onRefresh={loadDistributions}
					onSelect={openDetail}
					onCreate={() => (createOpen = true)}
				/>
			</TabsContent>
			<TabsContent value="cache" class="m-0">
				<CachePoliciesTab />
			</TabsContent>
			<TabsContent value="origin" class="m-0">
				<OriginRequestPoliciesTab />
			</TabsContent>
			<TabsContent value="oai" class="m-0">
				<OaiTab />
			</TabsContent>
			<TabsContent value="keyGroups" class="m-0">
				<KeyGroupsTab />
			</TabsContent>
			<TabsContent value="publicKeys" class="m-0">
				<PublicKeysTab />
			</TabsContent>
			<TabsContent value="functions" class="m-0">
				<FunctionsTab />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<CreateDistributionDialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={loadDistributions}
/>

<DistributionDetailSheet
	distribution={selected}
	open={detailOpen}
	onOpenChange={(o) => (detailOpen = o)}
/>
