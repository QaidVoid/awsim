<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { ResourceConsole, EmptyState } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import ToggleIcon from '@lucide/svelte/icons/toggle-left';
	import ApplicationsList from '$lib/components/appconfig/applications-list.svelte';
	import EnvironmentsTab from '$lib/components/appconfig/environments-tab.svelte';
	import ProfilesTab from '$lib/components/appconfig/profiles-tab.svelte';
	import DeploymentsTab from '$lib/components/appconfig/deployments-tab.svelte';
	import type { Application } from '$lib/api/appconfig';

	let selected = $state<Application | null>(null);
	let active: string = $state(
		useTab('appconfig', ['environments', 'profiles', 'deployments'] as const, 'environments', {
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
	title="AppConfig"
	description="Feature flags & configuration delivery — applications, environments, profiles, deployments."
	listWidth="260px"
	hasSelection={!!selected}
>
	{#snippet list()}
		<ApplicationsList
			selectedId={selected?.id ?? null}
			onSelect={(a) => (selected = a)}
			onChanged={refresh}
		/>
	{/snippet}

	{#snippet empty()}
		<div class="flex h-full items-center justify-center p-6">
			<EmptyState
				icon={ToggleIcon}
				title="No application selected"
				description="Pick an application on the left to manage its environments, profiles, and deployments."
			/>
		</div>
	{/snippet}

	{#if selected}
		<Tabs bind:value={active} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
			<TabsList variant="line" class="border-b border-border px-4">
				<TabsTrigger value="environments">Environments</TabsTrigger>
				<TabsTrigger value="profiles">Profiles</TabsTrigger>
				<TabsTrigger value="deployments">Deployments</TabsTrigger>
			</TabsList>

			<div class="min-h-0 flex-1 overflow-y-auto">
				<TabsContent value="environments" class="m-0">
					<EnvironmentsTab appId={selected.id} {refreshKey} />
				</TabsContent>
				<TabsContent value="profiles" class="m-0">
					<ProfilesTab appId={selected.id} {refreshKey} />
				</TabsContent>
				<TabsContent value="deployments" class="m-0">
					<DeploymentsTab appId={selected.id} {refreshKey} />
				</TabsContent>
			</div>
		</Tabs>
	{/if}
</ResourceConsole>
