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
	import { toast } from 'svelte-sonner';
	import { createDeliveryStream } from '$lib/api/firehose';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let bucketArn = $state('arn:aws:s3:::my-bucket');
	let roleArn = $state('arn:aws:iam::000000000000:role/firehose');
	let prefix = $state('raw/');
	let bufferSizeMb = $state(5);
	let bufferSeconds = $state(300);
	let creating = $state(false);

	async function submit() {
		if (!name.trim() || !bucketArn.trim() || !roleArn.trim()) {
			toast.error('Name, bucket ARN, and role ARN are required.');
			return;
		}
		creating = true;
		try {
			await createDeliveryStream({
				name: name.trim(),
				s3Destination: {
					bucketArn: bucketArn.trim(),
					roleArn: roleArn.trim(),
					prefix: prefix.trim() || undefined,
					bufferingHints: {
						sizeInMBs: bufferSizeMb,
						intervalInSeconds: bufferSeconds,
					},
					compressionFormat: 'UNCOMPRESSED',
				},
			});
			toast.success('Delivery stream created.');
			name = '';
			onOpenChange(false);
			onCreated?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New delivery stream</DialogTitle>
			<DialogDescription>
				Direct-put stream that buffers records and writes to S3.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="fh-create-name">Name</Label>
				<Input id="fh-create-name" bind:value={name} placeholder="my-firehose" />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="fh-create-bucket">S3 bucket ARN</Label>
				<Input id="fh-create-bucket" bind:value={bucketArn} />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="fh-create-role">IAM role ARN</Label>
				<Input id="fh-create-role" bind:value={roleArn} />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="fh-create-prefix">Object prefix</Label>
				<Input id="fh-create-prefix" bind:value={prefix} />
			</div>
			<div class="grid grid-cols-2 gap-3">
				<div class="flex flex-col gap-1">
					<Label for="fh-create-size">Buffer size (MB)</Label>
					<Input id="fh-create-size" type="number" min="1" max="128" bind:value={bufferSizeMb} />
				</div>
				<div class="flex flex-col gap-1">
					<Label for="fh-create-secs">Buffer interval (s)</Label>
					<Input id="fh-create-secs" type="number" min="60" max="900" bind:value={bufferSeconds} />
				</div>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !name.trim()}>
				{creating ? 'Creating…' : 'Create stream'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
