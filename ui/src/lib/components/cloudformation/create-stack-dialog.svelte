<script lang="ts">
	import { createStack } from '$lib/api/cloudformation';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
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

	const DEFAULT_TEMPLATE = `AWSTemplateFormatVersion: '2010-09-09'
Description: My stack
Resources:
  MyBucket:
    Type: AWS::S3::Bucket
`;

	let name = $state('');
	let body = $state(DEFAULT_TEMPLATE);
	let saving = $state(false);

	function reset() {
		name = '';
		body = DEFAULT_TEMPLATE;
	}

	async function handleSubmit(e: Event) {
		e.preventDefault();
		if (!name.trim() || !body.trim()) return;
		saving = true;
		try {
			await createStack({ stackName: name.trim(), templateBody: body });
			toast.success(`Stack ${name.trim()} created`);
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
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle>Create stack</DialogTitle>
			<DialogDescription>Provision resources from a CloudFormation template.</DialogDescription>
		</DialogHeader>
		<form onsubmit={handleSubmit} class="flex flex-col gap-4 py-2">
			<div class="flex flex-col gap-1.5">
				<Label for="cf-stack-name">Stack name</Label>
				<Input id="cf-stack-name" bind:value={name} placeholder="my-stack" required />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="cf-stack-template">Template body (YAML or JSON)</Label>
				<Textarea
					id="cf-stack-template"
					bind:value={body}
					rows={12}
					class="font-mono text-xs"
					required
				/>
			</div>
			<DialogFooter>
				<Button type="button" variant="ghost" onclick={() => onOpenChange(false)}>Cancel</Button>
				<Button type="submit" disabled={saving || !name.trim() || !body.trim()}>
					<Plus />
					{saving ? 'Creating...' : 'Create'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
