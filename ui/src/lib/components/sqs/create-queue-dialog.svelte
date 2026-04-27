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
	import { createQueue } from '$lib/api/sqs';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: (queueUrl: string) => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let fifo = $state(false);
	let contentDedup = $state(false);
	let visibilityTimeout = $state(30);
	let creating = $state(false);

	function reset() {
		name = '';
		fifo = false;
		contentDedup = false;
		visibilityTimeout = 30;
	}

	async function submit() {
		if (!name.trim()) {
			toast.error('Queue name is required.');
			return;
		}
		creating = true;
		try {
			const res = await createQueue({
				name: name.trim(),
				fifo,
				contentBasedDeduplication: fifo ? contentDedup : false,
				visibilityTimeout,
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
	<DialogContent class="sm:max-w-md">
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
			{/if}

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
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !name.trim()}>
				{creating ? 'Creating…' : 'Create queue'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
