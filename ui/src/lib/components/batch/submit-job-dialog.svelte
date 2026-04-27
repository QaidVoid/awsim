<script lang="ts">
	import { submitJob } from '$lib/api/batch';
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
		onSubmitted: () => void;
	}

	let { open, onOpenChange, onSubmitted }: Props = $props();

	let jobName = $state('');
	let jobQueue = $state('');
	let jobDefinition = $state('');
	let saving = $state(false);

	function reset() {
		jobName = '';
		jobQueue = '';
		jobDefinition = '';
	}

	async function handleSubmit(e: Event) {
		e.preventDefault();
		if (!jobName.trim() || !jobQueue.trim() || !jobDefinition.trim()) return;
		saving = true;
		try {
			await submitJob({
				jobName: jobName.trim(),
				jobQueue: jobQueue.trim(),
				jobDefinition: jobDefinition.trim()
			});
			toast.success(`Submitted ${jobName.trim()}`);
			reset();
			onOpenChange(false);
			onSubmitted();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Submit failed');
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
			<DialogTitle>Submit job</DialogTitle>
			<DialogDescription>Queue a new Batch job for execution.</DialogDescription>
		</DialogHeader>
		<form onsubmit={handleSubmit} class="flex flex-col gap-4 py-2">
			<div class="flex flex-col gap-1.5">
				<Label for="batch-job-name">Job name</Label>
				<Input id="batch-job-name" bind:value={jobName} placeholder="my-job" required />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="batch-job-queue">Job queue</Label>
				<Input
					id="batch-job-queue"
					bind:value={jobQueue}
					placeholder="my-queue or arn:aws:batch:..."
					required
				/>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="batch-job-def">Job definition</Label>
				<Input
					id="batch-job-def"
					bind:value={jobDefinition}
					placeholder="my-def:1"
					required
				/>
			</div>
			<DialogFooter>
				<Button type="button" variant="ghost" onclick={() => onOpenChange(false)}>Cancel</Button>
				<Button
					type="submit"
					disabled={saving || !jobName.trim() || !jobQueue.trim() || !jobDefinition.trim()}
				>
					<Plus />
					{saving ? 'Submitting...' : 'Submit'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
