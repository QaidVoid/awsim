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
	import { Textarea } from '$lib/components/ui/textarea';
	import { toast } from 'svelte-sonner';
	import { createSecret } from '$lib/api/secrets';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: (arn: string) => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let secretString = $state('');
	let description = $state('');
	let creating = $state(false);

	function reset() {
		name = '';
		secretString = '';
		description = '';
	}

	function loadJsonSample() {
		secretString = JSON.stringify(
			{ username: 'admin', password: 's3cr3t', host: 'db.internal' },
			null,
			2
		);
	}

	async function submit() {
		if (!name.trim()) {
			toast.error('Secret name is required.');
			return;
		}
		if (!secretString.trim()) {
			toast.error('Secret value cannot be empty.');
			return;
		}
		creating = true;
		try {
			const res = await createSecret(
				name.trim(),
				secretString,
				description.trim() || undefined
			);
			toast.success('Secret created.');
			reset();
			onOpenChange(false);
			onCreated?.(res.arn);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create secret');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle>New secret</DialogTitle>
			<DialogDescription>
				Store a credential or config value. The value becomes the secret's first
				version.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="sm-create-name">Name</Label>
				<Input
					id="sm-create-name"
					bind:value={name}
					placeholder="my/app/db-credentials"
					autocomplete="off"
					class="font-mono text-xs"
				/>
			</div>
			<div class="flex flex-col gap-1">
				<div class="flex items-center justify-between">
					<Label for="sm-create-value">Secret value</Label>
					<Button variant="ghost" size="xs" onclick={loadJsonSample}>Load JSON sample</Button>
				</div>
				<Textarea
					id="sm-create-value"
					bind:value={secretString}
					rows={6}
					class="font-mono text-xs"
					placeholder="Plain text or JSON"
				/>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="sm-create-desc">Description (optional)</Label>
				<Input id="sm-create-desc" bind:value={description} placeholder="optional" />
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !name.trim() || !secretString.trim()}>
				{creating ? 'Creating…' : 'Create secret'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
