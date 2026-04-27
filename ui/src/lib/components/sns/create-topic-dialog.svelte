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
	import { createTopic } from '$lib/api/sns';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: (arn: string) => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let fifo = $state(false);
	let creating = $state(false);

	function reset() {
		name = '';
		fifo = false;
	}

	async function submit() {
		if (!name.trim()) {
			toast.error('Topic name is required.');
			return;
		}
		creating = true;
		try {
			const res = await createTopic(name.trim(), fifo);
			toast.success('Topic created.');
			reset();
			onOpenChange(false);
			onCreated?.(res.topicArn);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create topic');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New SNS topic</DialogTitle>
			<DialogDescription>
				Topics fan out a published message to every subscriber.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="sns-create-name">Topic name</Label>
				<Input
					id="sns-create-name"
					bind:value={name}
					placeholder="my-topic"
					autocomplete="off"
				/>
			</div>

			<div class="flex items-center justify-between rounded-md border border-border px-3 py-2">
				<div>
					<Label for="sns-create-fifo" class="text-sm">FIFO topic</Label>
					<p class="text-[11px] text-muted-foreground">
						Strict ordering, no duplicates. Subscribers must be FIFO SQS queues.
					</p>
				</div>
				<Switch id="sns-create-fifo" bind:checked={fifo} />
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !name.trim()}>
				{creating ? 'Creating…' : 'Create topic'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
