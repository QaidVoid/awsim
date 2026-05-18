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
	import PolicyEditor from '$lib/components/iam/policy-editor.svelte';
	import { toast } from 'svelte-sonner';
	import { createPolicy, type Policy } from '$lib/api/organizations';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: (p: Policy) => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	const DEFAULT_SCP = JSON.stringify(
		{
			Version: '2012-10-17',
			Statement: [{ Sid: 'DenyExpensive', Effect: 'Deny', Action: ['ec2:RunInstances'], Resource: '*' }]
		},
		null,
		2
	);

	let name = $state('');
	let description = $state('');
	let content = $state(DEFAULT_SCP);
	let creating = $state(false);

	function reset() {
		name = '';
		description = '';
		content = DEFAULT_SCP;
	}

	async function submit() {
		if (!name.trim()) {
			toast.error('Policy name is required.');
			return;
		}
		try {
			JSON.parse(content);
		} catch {
			toast.error('Policy document is not valid JSON.');
			return;
		}
		creating = true;
		try {
			const p = await createPolicy(name.trim(), description.trim(), content);
			toast.success('SCP created.');
			reset();
			onOpenChange(false);
			onCreated?.(p);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create policy');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-2xl">
		<DialogHeader>
			<DialogTitle>New service control policy</DialogTitle>
			<DialogDescription>
				SCPs cap what member accounts can do. AWSim enforces them in the IAM engine -
				test the effect afterwards in IAM &gt; Simulator.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="grid grid-cols-2 gap-3">
				<div class="flex flex-col gap-1">
					<Label for="scp-name">Name</Label>
					<Input id="scp-name" bind:value={name} placeholder="DenyExpensiveServices" />
				</div>
				<div class="flex flex-col gap-1">
					<Label for="scp-desc">Description (optional)</Label>
					<Input id="scp-desc" bind:value={description} placeholder="optional" />
				</div>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="scp-doc">Policy document</Label>
				<PolicyEditor id="scp-doc" bind:value={content} rows={14} />
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !name.trim()}>
				{creating ? 'Creating…' : 'Create SCP'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
