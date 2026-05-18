<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { ResourceConsole, EmptyState } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import MegaphoneIcon from '@lucide/svelte/icons/megaphone';
	import AppsList from '$lib/components/pinpoint/apps-list.svelte';
	import EndpointsTab from '$lib/components/pinpoint/endpoints-tab.svelte';
	import SegmentsTab from '$lib/components/pinpoint/segments-tab.svelte';
	import CampaignsTab from '$lib/components/pinpoint/campaigns-tab.svelte';
	import type { App } from '$lib/api/pinpoint';

	let selected = $state<App | null>(null);
	let active: string = $state(
		useTab('pinpoint', ['endpoints', 'segments', 'campaigns'] as const, 'endpoints', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);
	let refreshKey = $state(0);

	function refresh() {
		refreshKey += 1;
	}
</script>

<ResourceConsole
	title="Pinpoint"
	description="Marketing apps, endpoints, segments, and campaigns. AWSim does not deliver real messages."
	listWidth="260px"
	hasSelection={!!selected}
>
	{#snippet list()}
		<AppsList
			selectedId={selected?.id ?? null}
			onSelect={(a) => (selected = a)}
			onChanged={refresh}
		/>
	{/snippet}

	{#snippet empty()}
		<div class="flex h-full items-center justify-center p-6">
			<EmptyState
				icon={MegaphoneIcon}
				title="No app selected"
				description="Pick an app on the left to manage its endpoints, segments, and campaigns."
			/>
		</div>
	{/snippet}

	{#if selected}
		<Tabs bind:value={active} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
			<TabsList variant="line" class="border-b border-border px-4">
				<TabsTrigger value="endpoints">Endpoints</TabsTrigger>
				<TabsTrigger value="segments">Segments</TabsTrigger>
				<TabsTrigger value="campaigns">Campaigns</TabsTrigger>
			</TabsList>

			<div class="min-h-0 flex-1 overflow-y-auto">
				<TabsContent value="endpoints" class="m-0">
					<EndpointsTab appId={selected.id} />
				</TabsContent>
				<TabsContent value="segments" class="m-0">
					<SegmentsTab appId={selected.id} {refreshKey} />
				</TabsContent>
				<TabsContent value="campaigns" class="m-0">
					<CampaignsTab appId={selected.id} {refreshKey} />
				</TabsContent>
			</div>
		</Tabs>
	{/if}
</ResourceConsole>
