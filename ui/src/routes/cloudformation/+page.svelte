<script lang="ts">
	/**
	 * CloudFormation page — stack workspace with two-pane layout.
	 *
	 * Left: list of stacks. Right: tabbed detail (resources, events,
	 * parameters, outputs, template).
	 */
	import { useTab } from '$lib/util/tab.svelte';
	import { onMount } from 'svelte';
	import {
		listStacks,
		describeStack,
		describeStackResources,
		describeStackEvents,
		deleteStack,
		type StackSummary,
		type Stack,
		type StackResource,
		type StackEvent
	} from '$lib/api/cloudformation';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import { Badge } from '$lib/components/ui/badge';
	import {
		StackList,
		ResourcesTab,
		EventsTab,
		ParametersTab,
		OutputsTab,
		TemplateViewer,
		StackDetailSheet,
		CreateStackDialog
	} from '$lib/components/cloudformation';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Info from '@lucide/svelte/icons/info';
	import Layers from '@lucide/svelte/icons/layers';
	import { EmptyState } from '$lib/components/service';
	import { toast } from 'svelte-sonner';

	let stacks = $state<StackSummary[]>([]);
	let stacksLoading = $state(true);

	let selectedName = $state<string | null>(null);
	let detail = $state<Stack | null>(null);
	let resources = $state<StackResource[]>([]);
	let events = $state<StackEvent[]>([]);
	let detailLoading = $state(false);
	let active: string = $state(
		useTab('cloudformation', ['resources', 'events', 'parameters', 'outputs', 'template'] as const, 'resources', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);

	let createOpen = $state(false);
	let sheetOpen = $state(false);

	async function loadStacks() {
		stacksLoading = true;
		try {
			const r = await listStacks();
			// Hide DELETE_COMPLETE noise.
			stacks = r.stacks.filter((s) => s.stackStatus !== 'DELETE_COMPLETE');
			if (selectedName && !stacks.some((s) => s.stackName === selectedName)) {
				selectedName = null;
				detail = null;
			}
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load stacks');
		} finally {
			stacksLoading = false;
		}
	}

	async function loadDetail(name: string) {
		detailLoading = true;
		try {
			const [d, r, e] = await Promise.all([
				describeStack(name),
				describeStackResources(name),
				describeStackEvents(name)
			]);
			detail = d;
			resources = r.resources;
			events = e.events;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load stack detail');
			detail = null;
			resources = [];
			events = [];
		} finally {
			detailLoading = false;
		}
	}

	function selectStack(s: StackSummary) {
		selectedName = s.stackName;
		loadDetail(s.stackName);
	}

	async function handleDelete() {
		if (!selectedName) return;
		if (!confirm(`Delete stack "${selectedName}"?`)) return;
		const name = selectedName;
		try {
			await deleteStack(name);
			toast.success(`Delete initiated for ${name}`);
			selectedName = null;
			detail = null;
			await loadStacks();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		}
	}

	onMount(loadStacks);
</script>

<svelte:head>
	<title>AWSim · CloudFormation</title>
</svelte:head>

<ServicePage
	title="CloudFormation"
	description="Model and provision AWS resources from declarative templates."
>
	{#snippet actions()}
		<Button type="button" variant="outline" size="sm" onclick={loadStacks} disabled={stacksLoading}>
			<RefreshCw class={stacksLoading ? 'animate-spin' : ''} />
			Refresh
		</Button>
		<Button type="button" size="sm" onclick={() => (createOpen = true)}>
			<Plus />
			Create stack
		</Button>
	{/snippet}

	<div class="grid h-full min-h-0 grid-cols-[20rem_minmax(0,1fr)]">
		<aside class="flex min-h-0 flex-col border-r border-border">
			<header class="flex shrink-0 items-center justify-between border-b border-border px-3 py-2">
				<span class="text-xs text-muted-foreground">
					{stacks.length} stack{stacks.length === 1 ? '' : 's'}
				</span>
			</header>
			<StackList
				{stacks}
				loading={stacksLoading}
				selected={selectedName}
				onSelect={selectStack}
			/>
		</aside>

		<section class="flex min-h-0 flex-col">
			{#if !selectedName}
				<div class="flex h-full items-center justify-center p-6">
					<EmptyState
						icon={Layers}
						title="Select a stack"
						description="Pick a stack on the left to view its resources, events, and template."
					/>
				</div>
			{:else}
				<header
					class="flex shrink-0 items-center justify-between gap-2 border-b border-border px-4 py-2"
				>
					<div class="flex items-center gap-2 truncate">
						<span class="truncate font-mono text-sm">{selectedName}</span>
						{#if detail}
							<Badge variant="outline">{detail.stackStatus}</Badge>
						{/if}
					</div>
					<div class="flex shrink-0 items-center gap-1">
						<Button
							type="button"
							variant="ghost"
							size="sm"
							onclick={() => (sheetOpen = true)}
							disabled={!detail}
						>
							<Info />
							Details
						</Button>
						<Button
							type="button"
							variant="ghost"
							size="sm"
							onclick={() => selectedName && loadDetail(selectedName)}
							disabled={detailLoading}
							aria-label="Refresh detail"
						>
							<RefreshCw class={detailLoading ? 'animate-spin' : ''} />
						</Button>
						<Button
							type="button"
							variant="ghost"
							size="sm"
							onclick={handleDelete}
							aria-label="Delete stack"
						>
							<Trash2 />
						</Button>
					</div>
				</header>

				<Tabs bind:value={active} class="flex min-h-0 flex-1 flex-col">
					<TabsList class="mx-4 mt-2 self-start">
						<TabsTrigger value="resources">Resources</TabsTrigger>
						<TabsTrigger value="events">Events</TabsTrigger>
						<TabsTrigger value="parameters">Parameters</TabsTrigger>
						<TabsTrigger value="outputs">Outputs</TabsTrigger>
						<TabsTrigger value="template">Template</TabsTrigger>
					</TabsList>
					<div class="min-h-0 flex-1">
						<TabsContent value="resources" class="m-0 h-full">
							<ResourcesTab {resources} loading={detailLoading} />
						</TabsContent>
						<TabsContent value="events" class="m-0 h-full">
							<EventsTab {events} loading={detailLoading} />
						</TabsContent>
						<TabsContent value="parameters" class="m-0 h-full">
							<ParametersTab parameters={detail?.parameters ?? []} />
						</TabsContent>
						<TabsContent value="outputs" class="m-0 h-full">
							<OutputsTab outputs={detail?.outputs ?? []} />
						</TabsContent>
						<TabsContent value="template" class="m-0 h-full">
							<TemplateViewer stackName={selectedName} />
						</TabsContent>
					</div>
				</Tabs>
			{/if}
		</section>
	</div>
</ServicePage>

<StackDetailSheet stack={detail} open={sheetOpen} onOpenChange={(o) => (sheetOpen = o)} />
<CreateStackDialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={loadStacks}
/>
