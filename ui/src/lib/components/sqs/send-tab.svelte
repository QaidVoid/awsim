<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import SendIcon from '@lucide/svelte/icons/send';
	import { toast } from 'svelte-sonner';
	import { sendMessage } from '$lib/api/sqs';

	interface Props {
		queueUrl: string;
		isFifo: boolean;
		onSent?: () => void;
	}

	let { queueUrl, isFifo, onSent }: Props = $props();

	let body = $state('');
	let delaySeconds = $state(0);
	let messageGroupId = $state('default');
	let messageDeduplicationId = $state('');
	let sending = $state(false);

	async function send() {
		if (!body.trim()) {
			toast.error('Message body cannot be empty.');
			return;
		}
		sending = true;
		try {
			await sendMessage({
				queueUrl,
				body,
				delaySeconds: isFifo ? undefined : delaySeconds || undefined,
				messageGroupId: isFifo ? messageGroupId.trim() || 'default' : undefined,
				messageDeduplicationId:
					isFifo && messageDeduplicationId.trim() ? messageDeduplicationId.trim() : undefined,
			});
			toast.success('Message sent.');
			body = '';
			onSent?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to send message');
		} finally {
			sending = false;
		}
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

	<div class="flex justify-end pt-2">
		<Button onclick={send} disabled={sending || !body.trim()}>
			<SendIcon />
			{sending ? 'Sending…' : 'Send message'}
		</Button>
	</div>
</div>
