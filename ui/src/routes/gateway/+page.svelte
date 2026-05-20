<script lang="ts">
	// Top-level page for the Model Gateway. Each sub-tab is its own
	// component so this file stays a thin tab router. Tab values map
	// 1:1 to ?tab= URL params via useTab.
	import { ServicePage, EmptyState } from '$lib/components/service';
	import {
		Tabs,
		TabsList,
		TabsTrigger,
		TabsContent,
	} from '$lib/components/ui/tabs';
	import { Button } from '$lib/components/ui/button';
	import { useTab } from '$lib/util/tab.svelte';
	import { route } from '$lib/url';
	import BackendsTab from '$lib/components/gateway/backends-tab.svelte';
	import CredentialsTab from '$lib/components/gateway/credentials-tab.svelte';
	import ModelsAliasesTab from '$lib/components/gateway/models-aliases-tab.svelte';
	import HealthTab from '$lib/components/gateway/health-tab.svelte';

	import Network from '@lucide/svelte/icons/network';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import Activity from '@lucide/svelte/icons/activity';
	import Boxes from '@lucide/svelte/icons/boxes';
	import SquareTerminal from '@lucide/svelte/icons/square-terminal';
	import GitFork from '@lucide/svelte/icons/git-fork';

	let active: string = $state(
		useTab(
			'gateway',
			['backends', 'credentials', 'models', 'routing', 'health', 'playground'] as const,
			'backends',
			{
				get: (): string => active,
				set: (v) => (active = v),
			},
		),
	);
</script>

<ServicePage
	title="Model Gateway"
	description="Provider-agnostic proxy in front of Bedrock InvokeModel / Converse / embeddings. Manage backends, credentials, model aliases, routing, and health here."
>
	<Tabs bind:value={active} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="backends">
				<Network class="mr-2 h-4 w-4" />Backends
			</TabsTrigger>
			<TabsTrigger value="credentials">
				<KeyRound class="mr-2 h-4 w-4" />Credentials
			</TabsTrigger>
			<TabsTrigger value="models">
				<Boxes class="mr-2 h-4 w-4" />Models &amp; Aliases
			</TabsTrigger>
			<TabsTrigger value="routing">
				<GitFork class="mr-2 h-4 w-4" />Routing
			</TabsTrigger>
			<TabsTrigger value="health">
				<Activity class="mr-2 h-4 w-4" />Health
			</TabsTrigger>
			<TabsTrigger value="playground">
				<SquareTerminal class="mr-2 h-4 w-4" />Playground
			</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="backends" class="m-0">
				<BackendsTab />
			</TabsContent>

			<TabsContent value="credentials" class="m-0">
				<CredentialsTab />
			</TabsContent>

			<TabsContent value="models" class="m-0">
				<ModelsAliasesTab />
			</TabsContent>

			<TabsContent value="routing" class="m-0">
				<div class="p-4">
					<EmptyState
						icon={GitFork}
						title="Coming in Phase 4-5"
						description="Automatic fallback on 5xx / timeout / rate-limit, per-target overrides (timeout, max tokens, temperature), and routing strategy (first / round-robin / least-latency)."
					/>
				</div>
			</TabsContent>

			<TabsContent value="health" class="m-0">
				<HealthTab />
			</TabsContent>

			<TabsContent value="playground" class="m-0">
				<div class="p-4">
					<EmptyState
						icon={SquareTerminal}
						title="Use the existing Bedrock playground"
						description="The in-place per-row tester lands in Phase 7. For now, the full playground lives under Bedrock."
						action={playgroundAction}
					/>
				</div>
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

{#snippet playgroundAction()}
	<Button variant="outline" size="sm" href={route('/bedrock') + '?tab=playground'}>
		Open Bedrock playground
	</Button>
{/snippet}
