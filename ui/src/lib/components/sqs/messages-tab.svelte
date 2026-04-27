<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import InboxIcon from '@lucide/svelte/icons/inbox';
	import { toast } from 'svelte-sonner';
	import {
		receiveMessages,
		deleteMessage,
		type Message,
	} from '$lib/api/sqs';

	interface Props {
		queueUrl: string;
		onSelect: (msg: Message) => void;
	}

	let { queueUrl, onSelect }: Props = $props();

	let messages = $state<Message[]>([]);
	let polling = $state(false);
	let maxMessages = $state(10);
	let waitTime = $state(0);
	let lastPolled = $state<Date | null>(null);

	async function poll() {
		polling = true;
		try {
			messages = await receiveMessages(queueUrl, maxMessages, waitTime);
			lastPolled = new Date();
			if (messages.length === 0) {
				toast.info('No messages available right now.');
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to receive messages');
		} finally {
			polling = false;
		}
	}

	async function handleDelete(msg: Message) {
		try {
			await deleteMessage(queueUrl, msg.receiptHandle);
			messages = messages.filter((m) => m.messageId !== msg.messageId);
			toast.success('Message deleted.');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete message');
		}
	}

	function preview(body: string): string {
		return body.length > 200 ? body.slice(0, 200) + '…' : body;
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex flex-wrap items-end gap-3 rounded-md border border-border bg-card/40 p-3">
		<div class="flex flex-col gap-1">
			<Label for="sqs-max-messages" class="text-xs">Max</Label>
			<Input
				id="sqs-max-messages"
				type="number"
				min="1"
				max="10"
				bind:value={maxMessages}
				class="h-8 w-20"
			/>
		</div>
		<div class="flex flex-col gap-1">
			<Label for="sqs-wait-time" class="text-xs">Wait (s)</Label>
			<Input
				id="sqs-wait-time"
				type="number"
				min="0"
				max="20"
				bind:value={waitTime}
				class="h-8 w-20"
			/>
		</div>
		<div class="flex-1"></div>
		<Button onclick={poll} disabled={polling} size="sm">
			<RefreshCwIcon class={polling ? 'animate-spin' : ''} />
			{polling ? 'Polling…' : 'Receive'}
		</Button>
	</div>

	{#if lastPolled}
		<p class="text-xs text-muted-foreground">
			Last poll: {lastPolled.toLocaleTimeString()} · {messages.length} message{messages.length === 1
				? ''
				: 's'}
		</p>
	{/if}

	{#if messages.length === 0}
		<EmptyState
			icon={InboxIcon}
			title="No messages received"
			description="Click Receive to poll the queue. Received messages remain invisible to other consumers for the visibility timeout."
		/>
	{:else}
		<ul class="flex flex-col gap-2">
			{#each messages as msg (msg.messageId)}
				<li class="rounded-md border border-border bg-card/40">
					<button
						type="button"
						class="block w-full px-3 py-2 text-left transition-colors hover:bg-muted/40"
						onclick={() => onSelect(msg)}
					>
						<div class="flex items-center justify-between gap-2">
							<span class="truncate font-mono text-[11px] text-muted-foreground">
								{msg.messageId}
							</span>
							<div class="flex items-center gap-1">
								{#if msg.attributes['ApproximateReceiveCount']}
									<Badge variant="outline" class="h-4 px-1.5 text-[10px]">
										received {msg.attributes['ApproximateReceiveCount']}×
									</Badge>
								{/if}
							</div>
						</div>
						<pre
							class="mt-2 max-h-32 overflow-hidden text-xs font-mono whitespace-pre-wrap break-all text-foreground">{preview(
								msg.body
							)}</pre>
					</button>
					<div class="flex justify-end border-t border-border/60 px-2 py-1">
						<Button
							size="xs"
							variant="ghost"
							onclick={() => handleDelete(msg)}
							class="text-destructive hover:text-destructive"
						>
							<Trash2Icon />
							Delete
						</Button>
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</div>
