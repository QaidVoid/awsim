<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import SendIcon from '@lucide/svelte/icons/send';
	import Inbox from '@lucide/svelte/icons/inbox';
	import { toast } from 'svelte-sonner';
	import { sendMessage } from '$lib/api/sqs';

	interface Props {
		queueUrl: string;
		isFifo: boolean;
		onSent?: () => void;
		/** Jump the page to this queue's Messages tab. */
		onViewMessages?: () => void;
	}

	let { queueUrl, isFifo, onSent, onViewMessages }: Props = $props();

	interface SendRun {
		id: number;
		ts: number;
		body: string;
		messageId: string;
		group?: string;
	}

	let body = $state('');
	let delaySeconds = $state(0);
	let messageGroupId = $state('default');
	let messageDeduplicationId = $state('');
	let sending = $state(false);
	let history = $state<SendRun[]>([]);
	let lastUrl = $state('');
	let nextId = 0;

	// History is per-queue: reset when the selected queue changes.
	$effect(() => {
		if (queueUrl !== lastUrl) {
			lastUrl = queueUrl;
			history = [];
		}
	});

	function relTime(ts: number): string {
		const s = Math.max(0, Math.round((Date.now() - ts) / 1000));
		if (s < 60) return `${s}s ago`;
		if (s < 3600) return `${Math.floor(s / 60)}m ago`;
		return `${Math.floor(s / 3600)}h ago`;
	}

	async function send() {
		if (!body.trim()) {
			toast.error('Message body cannot be empty.');
			return;
		}
		sending = true;
		try {
			const group = isFifo ? messageGroupId.trim() || 'default' : undefined;
			const r = await sendMessage({
				queueUrl,
				body,
				delaySeconds: isFifo ? undefined : delaySeconds || undefined,
				messageGroupId: group,
				messageDeduplicationId:
					isFifo && messageDeduplicationId.trim() ? messageDeduplicationId.trim() : undefined,
			});
			toast.success('Message sent.');
			const run: SendRun = {
				id: nextId++,
				ts: Date.now(),
				body,
				messageId: r.messageId,
				group
			};
			history = [run, ...history].slice(0, 20);
			body = '';
			onSent?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to send message');
		} finally {
			sending = false;
		}
	}

	function reuse(run: SendRun) {
		body = run.body;
		if (run.group) messageGroupId = run.group;
	}

	function loadSampleJson() {
		body = JSON.stringify(
			{ event: 'order.created', orderId: 'ORD-001', total: 12.5 },
			null,
			2
		);
	}
</script>

<div class="flex flex-col gap-3 p-4">
	{#if history.length}
		<section class="rounded-md border border-border bg-card">
			<header
				class="flex items-center justify-between border-b border-border px-3 py-2"
			>
				<h3 class="text-xs font-medium uppercase tracking-wide text-muted-foreground">
					Sent this session ({history.length})
				</h3>
				{#if onViewMessages}
					<button
						type="button"
						class="flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground"
						onclick={onViewMessages}
					>
						<Inbox class="size-3" />
						View in queue
					</button>
				{/if}
			</header>
			<div class="flex flex-col divide-y divide-border/60">
				{#each history as run (run.id)}
					<button
						type="button"
						onclick={() => reuse(run)}
						class="flex items-center gap-3 px-3 py-1.5 text-left text-xs hover:bg-muted/50"
						title="Reuse this body"
					>
						<span class="size-1.5 shrink-0 rounded-full bg-emerald-500"></span>
						<span class="min-w-0 flex-1 truncate font-mono">
							{run.body.replace(/\s+/g, ' ').trim()}
						</span>
						<span class="shrink-0 font-mono text-muted-foreground">{run.messageId.slice(0, 8)}</span>
						<span class="shrink-0 text-muted-foreground">{relTime(run.ts)}</span>
					</button>
				{/each}
			</div>
		</section>
	{/if}

	<div class="flex flex-col gap-2">
		<div class="flex items-center justify-between">
			<Label for="sqs-send-body">Body</Label>
			<Button variant="ghost" size="xs" onclick={loadSampleJson}>Load sample JSON</Button>
		</div>
		<Textarea
			id="sqs-send-body"
			bind:value={body}
			rows={10}
			class="font-mono text-xs"
			placeholder="Plain text or JSON…"
		/>
	</div>

	{#if isFifo}
		<div class="grid grid-cols-2 gap-3">
			<div class="flex flex-col gap-1">
				<Label for="sqs-group-id">Message group id</Label>
				<Input id="sqs-group-id" bind:value={messageGroupId} />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="sqs-dedup-id">Deduplication id</Label>
				<Input
					id="sqs-dedup-id"
					bind:value={messageDeduplicationId}
					placeholder="optional"
				/>
			</div>
		</div>
	{:else}
		<div class="flex flex-col gap-1">
			<Label for="sqs-delay">Delay seconds (0–900)</Label>
			<Input
				id="sqs-delay"
				type="number"
				min="0"
				max="900"
				bind:value={delaySeconds}
				class="w-32"
			/>
		</div>
	{/if}

	<div class="flex items-center justify-end gap-2 pt-2">
		{#if onViewMessages && history.length}
			<Button variant="outline" onclick={onViewMessages}>
				<Inbox />
				View in queue
			</Button>
		{/if}
		<Button onclick={send} disabled={sending || !body.trim()}>
			<SendIcon />
			{sending ? 'Sending…' : 'Send message'}
		</Button>
	</div>
</div>
