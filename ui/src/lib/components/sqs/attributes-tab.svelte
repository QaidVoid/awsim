<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import { toast } from 'svelte-sonner';
	import { setQueueAttributes, type QueueAttributes } from '$lib/api/sqs';

	interface Props {
		queueUrl: string;
		attrs: QueueAttributes;
		onSaved?: () => void;
	}

	let { queueUrl, attrs, onSaved }: Props = $props();

	let visibilityTimeout = $state(0);
	let messageRetentionPeriod = $state(0);
	let delaySeconds = $state(0);
	let receiveMessageWaitTimeSeconds = $state(0);
	let saving = $state(false);

	$effect(() => {
		visibilityTimeout = attrs.visibilityTimeout;
		messageRetentionPeriod = attrs.messageRetentionPeriod;
		delaySeconds = attrs.delaySeconds;
		receiveMessageWaitTimeSeconds = attrs.receiveMessageWaitTimeSeconds;
	});

	async function save() {
		saving = true;
		try {
			await setQueueAttributes(queueUrl, {
				VisibilityTimeout: String(visibilityTimeout),
				MessageRetentionPeriod: String(messageRetentionPeriod),
				DelaySeconds: String(delaySeconds),
				ReceiveMessageWaitTimeSeconds: String(receiveMessageWaitTimeSeconds),
			});
			toast.success('Queue attributes updated.');
			onSaved?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to update attributes');
		} finally {
			saving = false;
		}
	}
</script>

<div class="flex flex-col gap-4 p-4">
	<section class="rounded-md border border-border bg-card/40 p-4">
		<h3 class="mb-3 text-sm font-semibold">Identity</h3>
		<dl class="grid grid-cols-[140px_1fr] gap-x-4 gap-y-2 text-xs">
			<dt class="text-muted-foreground">Type</dt>
			<dd>
				{#if attrs.isFifo}
					<Badge variant="outline" class="h-4 px-1.5 text-[10px]">FIFO</Badge>
				{:else}
					<Badge variant="outline" class="h-4 px-1.5 text-[10px]">Standard</Badge>
				{/if}
			</dd>
			<dt class="text-muted-foreground">ARN</dt>
			<dd class="font-mono text-[11px] break-all">{attrs.arn || '—'}</dd>
			<dt class="text-muted-foreground">Created</dt>
			<dd>
				{attrs.createdTimestamp
					? new Date(parseInt(attrs.createdTimestamp, 10) * 1000).toLocaleString()
					: '—'}
			</dd>
			<dt class="text-muted-foreground">Messages</dt>
			<dd>
				<span class="font-medium">{attrs.approximateNumberOfMessages}</span> available ·
				<span class="text-amber-500">{attrs.approximateNumberOfMessagesNotVisible}</span> in flight ·
				<span class="text-muted-foreground"
					>{attrs.approximateNumberOfMessagesDelayed} delayed</span
				>
			</dd>
		</dl>
	</section>

	<section class="rounded-md border border-border bg-card/40 p-4">
		<h3 class="mb-3 text-sm font-semibold">Configuration</h3>
		<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
			<div class="flex flex-col gap-1">
				<Label for="sqs-attr-vis">Visibility timeout (s)</Label>
				<Input
					id="sqs-attr-vis"
					type="number"
					min="0"
					max="43200"
					bind:value={visibilityTimeout}
				/>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="sqs-attr-retention">Retention period (s)</Label>
				<Input
					id="sqs-attr-retention"
					type="number"
					min="60"
					max="1209600"
					bind:value={messageRetentionPeriod}
				/>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="sqs-attr-delay">Delivery delay (s)</Label>
				<Input
					id="sqs-attr-delay"
					type="number"
					min="0"
					max="900"
					bind:value={delaySeconds}
				/>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="sqs-attr-wait">Receive wait time (s)</Label>
				<Input
					id="sqs-attr-wait"
					type="number"
					min="0"
					max="20"
					bind:value={receiveMessageWaitTimeSeconds}
				/>
			</div>
		</div>
		<div class="mt-4 flex justify-end">
			<Button onclick={save} disabled={saving} size="sm">
				{saving ? 'Saving…' : 'Save changes'}
			</Button>
		</div>
	</section>
</div>
