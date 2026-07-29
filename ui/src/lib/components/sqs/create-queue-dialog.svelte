<script lang="ts">
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Switch } from '$lib/components/ui/switch';
	import { toast } from 'svelte-sonner';
	import { createQueue, listQueues, getQueueAttributes, type Queue } from '$lib/api/sqs';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: (queueUrl: string) => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	// Defaults match what AWS applies when an attribute is omitted, so a
	// queue created here behaves like one created with no attributes set.
	const DEFAULTS = {
		visibilityTimeout: 30,
		messageRetentionPeriod: 345600,
		delaySeconds: 0,
		maximumMessageSize: 262144,
		receiveMessageWaitTimeSeconds: 0,
		maxReceiveCount: 10,
	};

	let name = $state('');
	let fifo = $state(false);
	let contentDedup = $state(false);
	let highThroughput = $state(false);
	let visibilityTimeout = $state(DEFAULTS.visibilityTimeout);
	let messageRetentionPeriod = $state(DEFAULTS.messageRetentionPeriod);
	let delaySeconds = $state(DEFAULTS.delaySeconds);
	let maximumMessageSize = $state(DEFAULTS.maximumMessageSize);
	let receiveMessageWaitTimeSeconds = $state(DEFAULTS.receiveMessageWaitTimeSeconds);
	let dlqEnabled = $state(false);
	let dlqArn = $state('');
	let maxReceiveCount = $state(DEFAULTS.maxReceiveCount);
	let creating = $state(false);

	// Candidate dead-letter targets. A dead-letter queue must be the same
	// type as its source, so a FIFO queue can only redrive to a FIFO queue.
	let arnsByName = $state<{ name: string; arn: string }[]>([]);
	let eligibleDlqs = $derived(arnsByName.filter((q) => q.name.endsWith('.fifo') === fifo));

	$effect(() => {
		if (open) loadQueues();
	});

	async function loadQueues() {
		try {
			const list: Queue[] = await listQueues();
			const resolved = await Promise.all(
				list.map(async (q) => {
					try {
						const attrs = await getQueueAttributes(q.url);
						return { name: q.name, arn: attrs.arn };
					} catch {
						return { name: q.name, arn: '' };
					}
				}),
			);
			arnsByName = resolved.filter((q) => q.arn !== '');
		} catch {
			// The dialog still works without dead-letter suggestions.
			arnsByName = [];
		}
	}

	function reset() {
		name = '';
		fifo = false;
		contentDedup = false;
		highThroughput = false;
		visibilityTimeout = DEFAULTS.visibilityTimeout;
		messageRetentionPeriod = DEFAULTS.messageRetentionPeriod;
		delaySeconds = DEFAULTS.delaySeconds;
		maximumMessageSize = DEFAULTS.maximumMessageSize;
		receiveMessageWaitTimeSeconds = DEFAULTS.receiveMessageWaitTimeSeconds;
		dlqEnabled = false;
		dlqArn = '';
		maxReceiveCount = DEFAULTS.maxReceiveCount;
	}

	// Range checks live here so an out-of-bounds value is reported next to
	// the field rather than as an opaque InvalidAttributeValue afterwards.
	function validate(): string | null {
		if (!name.trim()) return 'Queue name is required.';
		if (visibilityTimeout < 0 || visibilityTimeout > 43200)
			return 'Visibility timeout must be between 0 and 43200 seconds.';
		if (messageRetentionPeriod < 60 || messageRetentionPeriod > 1209600)
			return 'Message retention period must be between 60 and 1209600 seconds.';
		if (delaySeconds < 0 || delaySeconds > 900)
			return 'Delivery delay must be between 0 and 900 seconds.';
		if (maximumMessageSize < 1024 || maximumMessageSize > 262144)
			return 'Maximum message size must be between 1024 and 262144 bytes.';
		if (receiveMessageWaitTimeSeconds < 0 || receiveMessageWaitTimeSeconds > 20)
			return 'Receive message wait time must be between 0 and 20 seconds.';
		if (dlqEnabled) {
			if (!dlqArn) return 'Select a dead-letter queue, or turn the redrive policy off.';
			if (maxReceiveCount < 1 || maxReceiveCount > 1000)
				return 'Maximum receives must be between 1 and 1000.';
		}
		return null;
	}

	async function submit() {
		const problem = validate();
		if (problem) {
			toast.error(problem);
			return;
		}
		creating = true;
		try {
			const res = await createQueue({
				name: name.trim(),
				fifo,
				contentBasedDeduplication: fifo ? contentDedup : false,
				visibilityTimeout,
				messageRetentionPeriod,
				delaySeconds,
				maximumMessageSize,
				receiveMessageWaitTimeSeconds,
				deadLetterTargetArn: dlqEnabled ? dlqArn : undefined,
				maxReceiveCount: dlqEnabled ? maxReceiveCount : undefined,
				deduplicationScope: fifo && highThroughput ? 'messageGroup' : undefined,
				fifoThroughputLimit: fifo && highThroughput ? 'perMessageGroupId' : undefined,
			});
			toast.success('Queue created.');
			reset();
			onOpenChange(false);
			onCreated?.(res.queueUrl);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create queue');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-lg max-h-[85vh] overflow-y-auto">
		<DialogHeader>
			<DialogTitle>New SQS queue</DialogTitle>
			<DialogDescription>
				FIFO queues guarantee ordering and exactly-once delivery within a message group.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="sqs-create-name">Queue name</Label>
				<Input
					id="sqs-create-name"
					bind:value={name}
					placeholder="my-queue"
					autocomplete="off"
				/>
				<p class="text-[11px] text-muted-foreground">
					{fifo ? '`.fifo` suffix is added automatically.' : 'Letters, digits, hyphens.'}
				</p>
			</div>

			<div class="flex items-center justify-between rounded-md border border-border px-3 py-2">
				<div>
					<Label for="sqs-create-fifo" class="text-sm">FIFO queue</Label>
					<p class="text-[11px] text-muted-foreground">Strict ordering, no duplicates.</p>
				</div>
				<Switch id="sqs-create-fifo" bind:checked={fifo} />
			</div>

			{#if fifo}
				<div class="flex items-center justify-between rounded-md border border-border px-3 py-2">
					<div>
						<Label for="sqs-create-dedup" class="text-sm">Content-based dedup</Label>
						<p class="text-[11px] text-muted-foreground">
							Hash body for deduplication id.
						</p>
					</div>
					<Switch id="sqs-create-dedup" bind:checked={contentDedup} />
				</div>

				<div class="flex items-center justify-between rounded-md border border-border px-3 py-2">
					<div>
						<Label for="sqs-create-htp" class="text-sm">High throughput FIFO</Label>
						<p class="text-[11px] text-muted-foreground">
							Per-message-group dedup scope and throughput limit.
						</p>
					</div>
					<Switch id="sqs-create-htp" bind:checked={highThroughput} />
				</div>
			{/if}

			<p class="pt-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
				Configuration
			</p>

			<div class="grid grid-cols-2 gap-3">
				<div class="flex flex-col gap-1">
					<Label for="sqs-create-vis">Visibility timeout (s)</Label>
					<Input
						id="sqs-create-vis"
						type="number"
						min="0"
						max="43200"
						bind:value={visibilityTimeout}
					/>
				</div>
				<div class="flex flex-col gap-1">
					<Label for="sqs-create-ret">Retention period (s)</Label>
					<Input
						id="sqs-create-ret"
						type="number"
						min="60"
						max="1209600"
						bind:value={messageRetentionPeriod}
					/>
				</div>
				<div class="flex flex-col gap-1">
					<Label for="sqs-create-delay">Delivery delay (s)</Label>
					<Input
						id="sqs-create-delay"
						type="number"
						min="0"
						max="900"
						bind:value={delaySeconds}
					/>
				</div>
				<div class="flex flex-col gap-1">
					<Label for="sqs-create-size">Max message size (bytes)</Label>
					<Input
						id="sqs-create-size"
						type="number"
						min="1024"
						max="262144"
						bind:value={maximumMessageSize}
					/>
				</div>
				<div class="col-span-2 flex flex-col gap-1">
					<Label for="sqs-create-wait">Receive message wait time (s)</Label>
					<Input
						id="sqs-create-wait"
						type="number"
						min="0"
						max="20"
						bind:value={receiveMessageWaitTimeSeconds}
					/>
					<p class="text-[11px] text-muted-foreground">
						Above 0 enables long polling, so ReceiveMessage waits for a message instead of
						returning empty straight away.
					</p>
				</div>
			</div>

			<p class="pt-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
				Dead-letter queue
			</p>

			<div class="flex items-center justify-between rounded-md border border-border px-3 py-2">
				<div>
					<Label for="sqs-create-dlq" class="text-sm">Redrive policy</Label>
					<p class="text-[11px] text-muted-foreground">
						Move messages aside after repeated failed receives.
					</p>
				</div>
				<Switch id="sqs-create-dlq" bind:checked={dlqEnabled} />
			</div>

			{#if dlqEnabled}
				{#if eligibleDlqs.length === 0}
					<p class="text-[11px] text-muted-foreground">
						No eligible {fifo ? 'FIFO' : 'standard'} queue to redrive to. A dead-letter queue
						must be the same type as its source, so create one first.
					</p>
				{:else}
					<div class="grid grid-cols-2 gap-3">
						<div class="col-span-2 flex flex-col gap-1">
							<Label for="sqs-create-dlq-arn">Dead-letter queue</Label>
							<select
								id="sqs-create-dlq-arn"
								bind:value={dlqArn}
								class="h-9 rounded-md border border-border bg-background px-2 text-sm"
							>
								<option value="">Select a queue</option>
								{#each eligibleDlqs as q (q.arn)}
									<option value={q.arn}>{q.name}</option>
								{/each}
							</select>
						</div>
						<div class="flex flex-col gap-1">
							<Label for="sqs-create-mrc">Maximum receives</Label>
							<Input
								id="sqs-create-mrc"
								type="number"
								min="1"
								max="1000"
								bind:value={maxReceiveCount}
							/>
						</div>
					</div>
				{/if}
			{/if}
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !name.trim()}>
				{creating ? 'Creating...' : 'Create queue'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
