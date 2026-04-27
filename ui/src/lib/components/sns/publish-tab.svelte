<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Switch } from '$lib/components/ui/switch';
	import SendIcon from '@lucide/svelte/icons/send';
	import { toast } from 'svelte-sonner';
	import { publish } from '$lib/api/sns';

	interface Props {
		topicArn: string;
		isFifo: boolean;
	}

	let { topicArn, isFifo }: Props = $props();

	let subject = $state('');
	let message = $state('');
	let asJson = $state(false);
	let messageGroupId = $state('default');
	let messageDeduplicationId = $state('');
	let publishing = $state(false);

	async function send() {
		if (!message.trim()) {
			toast.error('Message body cannot be empty.');
			return;
		}
		publishing = true;
		try {
			const res = await publish({
				topicArn,
				message,
				subject: subject.trim() || undefined,
				messageStructure: asJson ? 'json' : undefined,
				messageGroupId: isFifo ? messageGroupId.trim() || 'default' : undefined,
				messageDeduplicationId:
					isFifo && messageDeduplicationId.trim() ? messageDeduplicationId.trim() : undefined,
			});
			toast.success(`Published. id=${res.messageId.slice(0, 8)}…`);
			message = '';
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Publish failed');
		} finally {
			publishing = false;
		}
	}

	function loadJsonSample() {
		message = JSON.stringify(
			{
				default: 'plain default message',
				email: 'rich email message',
				sqs: '{"event":"order.created"}',
			},
			null,
			2
		);
		asJson = true;
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex flex-col gap-1">
		<Label for="sns-pub-subject">Subject (optional)</Label>
		<Input id="sns-pub-subject" bind:value={subject} maxlength={100} />
	</div>

	<div class="flex flex-col gap-2">
		<div class="flex items-center justify-between">
			<Label for="sns-pub-body">Message body</Label>
			<div class="flex items-center gap-3">
				<label class="flex items-center gap-2 text-xs text-muted-foreground" for="sns-pub-json">
					<Switch id="sns-pub-json" bind:checked={asJson} size="sm" />
					Per-protocol JSON
				</label>
				<Button variant="ghost" size="xs" onclick={loadJsonSample}>Load sample</Button>
			</div>
		</div>
		<Textarea
			id="sns-pub-body"
			bind:value={message}
			rows={10}
			class="font-mono text-xs"
			placeholder={asJson
				? '{"default": "...", "sqs": "..."}'
				: 'Plain text or JSON…'}
		/>
	</div>

	{#if isFifo}
		<div class="grid grid-cols-2 gap-3">
			<div class="flex flex-col gap-1">
				<Label for="sns-pub-group">Message group id</Label>
				<Input id="sns-pub-group" bind:value={messageGroupId} />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="sns-pub-dedup">Deduplication id</Label>
				<Input id="sns-pub-dedup" bind:value={messageDeduplicationId} placeholder="optional" />
			</div>
		</div>
	{/if}

	<div class="flex justify-end pt-2">
		<Button onclick={send} disabled={publishing || !message.trim()}>
			<SendIcon />
			{publishing ? 'Publishing…' : 'Publish'}
		</Button>
	</div>
</div>
