<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { onMount } from 'svelte';
	import { ServicePage, EmptyState, ListSkeleton } from '$lib/components/service';
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
	import EraserIcon from '@lucide/svelte/icons/eraser';
	import InboxIcon from '@lucide/svelte/icons/inbox';
	import { toast } from 'svelte-sonner';
	import {
		listQueues,
		getQueueAttributes,
		deleteQueue,
		purgeQueue,
		type Queue,
		type QueueAttributes,
		type Message,
	} from '$lib/api/sqs';
	import QueueList from '$lib/components/sqs/queue-list.svelte';
	import MessagesTab from '$lib/components/sqs/messages-tab.svelte';
	import SendTab from '$lib/components/sqs/send-tab.svelte';
	import AttributesTab from '$lib/components/sqs/attributes-tab.svelte';
	import DlqTab from '$lib/components/sqs/dlq-tab.svelte';
	import MessageDetailSheet from '$lib/components/sqs/message-detail-sheet.svelte';
	import CreateQueueDialog from '$lib/components/sqs/create-queue-dialog.svelte';

	let queues = $state<Queue[]>([]);
	let attrsByUrl = $state<Record<string, QueueAttributes>>({});
	let loading = $state(true);
	let error = $state<string | null>(null);

	let selectedUrl = $state<string | null>(null);
	let active: string = $state(
		useTab('sqs', ['messages', 'send', 'attributes', 'dlq'] as const, 'messages', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);
	let detailMessage = $state<Message | null>(null);
	let detailOpen = $state(false);

	let createOpen = $state(false);
	let confirmAction = $state<{ type: 'delete' | 'purge'; url: string; name: string } | null>(
		null
	);

	let selectedQueue = $derived(queues.find((q) => q.url === selectedUrl) ?? null);
	let selectedAttrs = $derived(selectedUrl ? (attrsByUrl[selectedUrl] ?? null) : null);

	async function refreshAttrs(url: string) {
		try {
			const a = await getQueueAttributes(url);
			attrsByUrl = { ...attrsByUrl, [url]: a };
		} catch {
			/* ignore */
		}
	}

	async function loadAll() {
		loading = true;
		error = null;
		try {
			const list = await listQueues();
			queues = list;
			if (selectedUrl && !list.some((q) => q.url === selectedUrl)) {
				selectedUrl = null;
			}
			await Promise.all(list.map((q) => refreshAttrs(q.url)));
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load queues';
		} finally {
			loading = false;
		}
	}

	function handleSelect(url: string) {
		selectedUrl = url;
		active = 'messages';
	}

	async function handleDelete() {
		if (!confirmAction) return;
		const { url, type, name } = confirmAction;
		confirmAction = null;
		try {
			if (type === 'delete') {
				await deleteQueue(url);
				toast.success(`Queue ${name} deleted.`);
				if (selectedUrl === url) selectedUrl = null;
			} else {
				await purgeQueue(url);
				toast.success(`Queue ${name} purged.`);
			}
			await loadAll();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Operation failed');
		}
	}

	function openMessage(msg: Message) {
		detailMessage = msg;
		detailOpen = true;
	}

	onMount(loadAll);
</script>

<ServicePage
	title="SQS"
	description="Simple Queue Service. Send, receive, and manage messages."
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={loadAll} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<PlusIcon />
			New queue
		</Button>
	{/snippet}

	{#if error}
		<div class="px-6 py-4 text-sm text-destructive">{error}</div>
	{:else if loading && queues.length === 0}
		<div class="px-6 py-6">
			<ListSkeleton rows={6} />
		</div>
	{:else if queues.length === 0}
		<div class="px-6 py-12">
			<EmptyState
				icon={InboxIcon}
				title="No SQS queues"
				description="Create a standard or FIFO queue to start sending messages."
			>
				{#snippet action()}
					<Button onclick={() => (createOpen = true)}>
						<PlusIcon />
						Create queue
					</Button>
				{/snippet}
			</EmptyState>
		</div>
	{:else}
		<div class="grid h-full min-h-0 grid-cols-[280px_1fr]">
			<QueueList
				{queues}
				selectedUrl={selectedUrl}
				{attrsByUrl}
				onSelect={handleSelect}
				onCreate={() => (createOpen = true)}
			/>

			<div class="flex h-full min-h-0 flex-col overflow-hidden">
				{#if selectedQueue && selectedAttrs}
					<header
						class="flex items-center justify-between gap-3 border-b border-border px-5 py-3"
					>
						<div class="min-w-0">
							<div class="flex items-center gap-2">
								<h2 class="truncate font-mono text-sm font-medium">
									{selectedQueue.name}
								</h2>
								{#if selectedAttrs.isFifo}
									<Badge variant="outline" class="h-4 px-1.5 text-[10px]">FIFO</Badge>
								{/if}
								{#if selectedAttrs.redrivePolicy}
									<Badge variant="outline" class="h-4 px-1.5 text-[10px]">DLQ</Badge>
								{/if}
							</div>
							<p class="mt-0.5 truncate text-[11px] text-muted-foreground">
								{selectedAttrs.approximateNumberOfMessages} message(s) ·
								{selectedAttrs.approximateNumberOfMessagesNotVisible} in flight
							</p>
						</div>
						<div class="flex items-center gap-2">
							<Button
								size="sm"
								variant="outline"
								onclick={() =>
									(confirmAction = {
										type: 'purge',
										url: selectedQueue!.url,
										name: selectedQueue!.name,
									})}
							>
								<EraserIcon />
								Purge
							</Button>
							<Button
								size="sm"
								variant="destructive"
								onclick={() =>
									(confirmAction = {
										type: 'delete',
										url: selectedQueue!.url,
										name: selectedQueue!.name,
									})}
							>
								<Trash2Icon />
								Delete
							</Button>
						</div>
					</header>

					<Tabs bind:value={active} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
						<TabsList variant="line" class="border-b border-border px-4">
							<TabsTrigger value="messages">Messages</TabsTrigger>
							<TabsTrigger value="send">Send</TabsTrigger>
							<TabsTrigger value="attributes">Attributes</TabsTrigger>
							<TabsTrigger value="dlq">DLQ</TabsTrigger>
						</TabsList>

						<div class="min-h-0 flex-1 overflow-y-auto">
							<TabsContent value="messages" class="m-0">
								<MessagesTab queueUrl={selectedQueue.url} onSelect={openMessage} />
							</TabsContent>
							<TabsContent value="send" class="m-0">
								<SendTab
									queueUrl={selectedQueue.url}
									isFifo={selectedAttrs.isFifo}
									onSent={() => refreshAttrs(selectedQueue!.url)}
								/>
							</TabsContent>
							<TabsContent value="attributes" class="m-0">
								<AttributesTab
									queueUrl={selectedQueue.url}
									attrs={selectedAttrs}
									onSaved={() => refreshAttrs(selectedQueue!.url)}
								/>
							</TabsContent>
							<TabsContent value="dlq" class="m-0">
								<DlqTab
									current={selectedQueue}
									attrs={selectedAttrs}
									{queues}
									{attrsByUrl}
									onRedriven={() => loadAll()}
								/>
							</TabsContent>
						</div>
					</Tabs>
				{:else}
					<div class="flex h-full items-center justify-center text-sm text-muted-foreground">
						Select a queue to inspect.
					</div>
				{/if}
			</div>
		</div>
	{/if}
</ServicePage>

<CreateQueueDialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={() => loadAll()}
/>

<MessageDetailSheet
	open={detailOpen}
	queueUrl={selectedQueue?.url ?? ''}
	message={detailMessage}
	onOpenChange={(o) => (detailOpen = o)}
	onDeleted={() => selectedQueue && refreshAttrs(selectedQueue.url)}
/>

<Dialog
	open={confirmAction !== null}
	onOpenChange={(o) => {
		if (!o) confirmAction = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>
				{confirmAction?.type === 'delete' ? 'Delete queue?' : 'Purge queue?'}
			</DialogTitle>
			<DialogDescription>
				{#if confirmAction?.type === 'delete'}
					This permanently removes <span class="font-mono">{confirmAction.name}</span> and
					all of its messages.
				{:else}
					Removes all messages from <span class="font-mono">{confirmAction?.name}</span>. The
					queue itself is kept.
				{/if}
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmAction = null)}>Cancel</Button>
			<Button
				variant={confirmAction?.type === 'delete' ? 'destructive' : 'default'}
				onclick={handleDelete}
			>
				{confirmAction?.type === 'delete' ? 'Delete' : 'Purge'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
