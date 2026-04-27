<script lang="ts">
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { createBucket } from '$lib/api/s3';
	import { toast } from 'svelte-sonner';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		open: boolean;
		onClose: () => void;
		onCreated: (name: string) => void;
	}

	let { open = $bindable(false), onClose, onCreated }: Props = $props();

	let name = $state('');
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!open) {
			name = '';
			error = null;
			saving = false;
		}
	});

	async function submit() {
		const trimmed = name.trim();
		if (!trimmed) {
			error = 'Bucket name is required';
			return;
		}
		saving = true;
		error = null;
		try {
			await createBucket(trimmed);
			toast.success(`Created bucket ${trimmed}`);
			onCreated(trimmed);
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Failed to create bucket';
			error = msg;
			toast.error(msg);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Create bucket</DialogTitle>
			<DialogDescription>Bucket names must be unique within the region.</DialogDescription>
		</DialogHeader>
		<form
			class="flex flex-col gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			<div class="flex flex-col gap-1.5">
				<Label for="new-bucket-name">Bucket name</Label>
				<Input
					id="new-bucket-name"
					bind:value={name}
					placeholder="my-bucket-name"
					autocomplete="off"
				/>
			</div>
			{#if error}
				<p class="text-xs text-destructive">{error}</p>
			{/if}
			<DialogFooter>
				<Button type="button" variant="outline" onclick={onClose} disabled={saving}>
					Cancel
				</Button>
				<Button type="submit" disabled={saving || !name.trim()}>
					{#if saving}
						<Loader2 class="size-3.5 animate-spin" />
					{/if}
					Create
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
