<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { onMount } from 'svelte';
	import { pendingAction } from '$lib/pending-action.svelte';
	import { ResourceConsole, EmptyState } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import {
		Tabs,
		TabsList,
		TabsTrigger,
		TabsContent,
	} from '$lib/components/ui/tabs';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import MegaphoneIcon from '@lucide/svelte/icons/megaphone';
	import { toast } from 'svelte-sonner';
	import {
		listTopics,
		getTopicAttributes,
		deleteTopic,
		type Topic,
		type TopicAttributes,
	} from '$lib/api/sns';
	import TopicList from '$lib/components/sns/topic-list.svelte';
	import SubscriptionsTab from '$lib/components/sns/subscriptions-tab.svelte';
	import PublishTab from '$lib/components/sns/publish-tab.svelte';
	import AttributesTab from '$lib/components/sns/attributes-tab.svelte';
	import CreateTopicDialog from '$lib/components/sns/create-topic-dialog.svelte';

	let topics = $state<Topic[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	let selectedArn = $state<string | null>(null);
	let selectedAttrs = $state<TopicAttributes | null>(null);
	let attrsLoading = $state(false);
	let active: string = $state(
		useTab('sns', ['subs', 'publish', 'attrs'] as const, 'subs', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);

	let createOpen = $state(false);

	onMount(() => {
		if (pendingAction.consume('new-topic')) createOpen = true;
	});
	let confirmDelete = $state<{ arn: string; name: string } | null>(null);

	let selectedTopic = $derived(topics.find((t) => t.arn === selectedArn) ?? null);

	async function loadTopics() {
		loading = true;
		error = null;
		try {
			topics = await listTopics();
			if (selectedArn && !topics.some((t) => t.arn === selectedArn)) {
				selectedArn = null;
				selectedAttrs = null;
			}
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load topics';
		} finally {
			loading = false;
		}
	}

	async function loadAttrs(arn: string) {
		attrsLoading = true;
		try {
			selectedAttrs = await getTopicAttributes(arn);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load attributes');
			selectedAttrs = null;
		} finally {
			attrsLoading = false;
		}
	}

	function handleSelect(arn: string) {
		selectedArn = arn;
		selectedAttrs = null;
		active = 'subs';
		loadAttrs(arn);
	}

	async function handleDelete() {
		if (!confirmDelete) return;
		const { arn, name } = confirmDelete;
		confirmDelete = null;
		try {
			await deleteTopic(arn);
			toast.success(`Topic ${name} deleted.`);
			if (selectedArn === arn) {
				selectedArn = null;
				selectedAttrs = null;
			}
			await loadTopics();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}

	onMount(loadTopics);
</script>

<ResourceConsole
	title="SNS"
	description="Simple Notification Service. Publish to topics, fan out to subscribers."
	listError={error}
	onListRetry={loadTopics}
	listLoading={loading}
	listIsEmpty={topics.length === 0}
	listSkeletonRows={6}
	listWidth="280px"
	hasSelection={!!selectedTopic}
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={loadTopics} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<PlusIcon />
			New topic
		</Button>
	{/snippet}

	{#snippet listEmpty()}
		<EmptyState
			icon={MegaphoneIcon}
			title="No SNS topics"
			description="Create a topic to publish messages and fan out to subscribers."
		>
			{#snippet action()}
				<Button onclick={() => (createOpen = true)}>
					<PlusIcon />
					Create topic
				</Button>
			{/snippet}
		</EmptyState>
	{/snippet}

	{#snippet list()}
		<TopicList {topics} {selectedArn} onSelect={handleSelect} onCreate={() => (createOpen = true)} />
	{/snippet}

	{#snippet empty()}
		<div class="flex h-full items-center justify-center text-sm text-muted-foreground">
			Select a topic to inspect.
		</div>
	{/snippet}

	{#snippet detailHeader()}
		{#if selectedTopic}
			<header
				class="flex items-center justify-between gap-3 border-b border-border px-5 py-3"
			>
				<div class="min-w-0">
					<div class="flex items-center gap-2">
						<h2 class="truncate font-mono text-sm font-medium">{selectedTopic.name}</h2>
						{#if selectedAttrs?.isFifo}
							<Badge variant="outline" class="h-4 px-1.5 text-[10px]">FIFO</Badge>
						{/if}
					</div>
					<p class="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">
						{selectedTopic.arn}
					</p>
				</div>
				<Button
					size="sm"
					variant="destructive"
					onclick={() =>
						(confirmDelete = { arn: selectedTopic!.arn, name: selectedTopic!.name })}
				>
					<Trash2Icon />
					Delete
				</Button>
			</header>
		{/if}
	{/snippet}

	{#if selectedTopic}
		<Tabs bind:value={active} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
			<TabsList variant="line" class="border-b border-border px-4">
				<TabsTrigger value="subs">Subscriptions</TabsTrigger>
				<TabsTrigger value="publish">Publish</TabsTrigger>
				<TabsTrigger value="attrs">Attributes</TabsTrigger>
			</TabsList>

			<div class="min-h-0 flex-1 overflow-y-auto">
				<TabsContent value="subs" class="m-0">
					<SubscriptionsTab topicArn={selectedTopic.arn} />
				</TabsContent>
				<TabsContent value="publish" class="m-0">
					<PublishTab
						topicArn={selectedTopic.arn}
						isFifo={selectedAttrs?.isFifo ?? false}
						onViewSubscriptions={() => (active = 'subs')}
					/>
				</TabsContent>
				<TabsContent value="attrs" class="m-0">
					{#if attrsLoading || !selectedAttrs}
						<p class="px-4 py-4 text-xs text-muted-foreground">Loading attributes…</p>
					{:else}
						<AttributesTab attrs={selectedAttrs} />
					{/if}
				</TabsContent>
			</div>
		</Tabs>
	{/if}
</ResourceConsole>

<CreateTopicDialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={() => loadTopics()}
/>

<Dialog
	open={confirmDelete !== null}
	onOpenChange={(o) => {
		if (!o) confirmDelete = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Delete topic?</DialogTitle>
			<DialogDescription>
				This permanently removes <span class="font-mono">{confirmDelete?.name}</span> and all
				of its subscriptions.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmDelete = null)}>Cancel</Button>
			<Button variant="destructive" onclick={handleDelete}>Delete</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
