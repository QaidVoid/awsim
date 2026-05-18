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
	import { createKey } from '$lib/api/kms';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: (keyId: string) => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let description = $state('');
	let creating = $state(false);

	async function submit() {
		creating = true;
		try {
			const k = await createKey(description.trim() || undefined);
			toast.success('Key created.');
			description = '';
			onOpenChange(false);
			onCreated?.(k.keyId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create key');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New KMS key</DialogTitle>
			<DialogDescription>
				A symmetric ENCRYPT_DECRYPT customer-managed key. Add an alias later for a
				friendly name.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="kms-create-desc">Description (optional)</Label>
				<Input
					id="kms-create-desc"
					bind:value={description}
					placeholder="App data encryption key"
				/>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating}>
				{creating ? 'Creating…' : 'Create key'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
