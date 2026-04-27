<script lang="ts">
	import { createLocationS3 } from '$lib/api/datasync';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter
	} from '$lib/components/ui/dialog';
	import { toast } from 'svelte-sonner';
	import Plus from '@lucide/svelte/icons/plus';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	const DEFAULT_ROLE = 'arn:aws:iam::000000000000:role/datasync-s3';

	let bucketArn = $state('');
	let subdirectory = $state('/');
	let role = $state(DEFAULT_ROLE);
	let saving = $state(false);

	function reset() {
		bucketArn = '';
		subdirectory = '/';
		role = DEFAULT_ROLE;
	}

	async function handleSubmit(e: Event) {
		e.preventDefault();
		if (!bucketArn.trim() || !role.trim()) return;
		saving = true;
		try {
			await createLocationS3({
				s3BucketArn: bucketArn.trim(),
				subdirectory: subdirectory.trim() || '/',
				bucketAccessRoleArn: role.trim()
			});
			toast.success('S3 location created');
			reset();
			onOpenChange(false);
			onCreated();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Create failed');
		} finally {
			saving = false;
		}
	}
</script>

<Dialog
	{open}
	onOpenChange={(o) => {
		onOpenChange(o);
		if (!o) reset();
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Create S3 location</DialogTitle>
			<DialogDescription>Register an S3 bucket as a DataSync location.</DialogDescription>
		</DialogHeader>
		<form onsubmit={handleSubmit} class="flex flex-col gap-4 py-2">
			<div class="flex flex-col gap-1.5">
				<Label for="ds-bucket-arn">S3 bucket ARN</Label>
				<Input
					id="ds-bucket-arn"
					bind:value={bucketArn}
					placeholder="arn:aws:s3:::my-bucket"
					required
				/>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="ds-subdir">Subdirectory</Label>
				<Input id="ds-subdir" bind:value={subdirectory} placeholder="/" />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="ds-role">Bucket access role ARN</Label>
				<Input id="ds-role" bind:value={role} required />
			</div>
			<DialogFooter>
				<Button type="button" variant="ghost" onclick={() => onOpenChange(false)}>Cancel</Button>
				<Button type="submit" disabled={saving || !bucketArn.trim() || !role.trim()}>
					<Plus />
					{saving ? 'Creating...' : 'Create'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
