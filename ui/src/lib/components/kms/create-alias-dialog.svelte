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
	import {
		Select,
		SelectContent,
		SelectItem,
		SelectTrigger
	} from '$lib/components/ui/select';
	import { toast } from 'svelte-sonner';
	import { createAlias, listKeys, type Key } from '$lib/api/kms';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let aliasName = $state('');
	let targetKeyId = $state('');
	let keys = $state<Key[]>([]);
	let creating = $state(false);

	$effect(() => {
		if (open && keys.length === 0) {
			listKeys()
				.then((k) => (keys = k))
				.catch(() => {});
		}
	});

	async function submit() {
		if (!aliasName.trim()) {
			toast.error('Alias name is required.');
			return;
		}
		if (!targetKeyId) {
			toast.error('Pick a target key.');
			return;
		}
		creating = true;
		try {
			await createAlias(aliasName.trim(), targetKeyId);
			toast.success('Alias created.');
			aliasName = '';
			targetKeyId = '';
			onOpenChange(false);
			onCreated?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create alias');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New alias</DialogTitle>
			<DialogDescription>
				A friendly name pointing at a KMS key. The <span class="font-mono">alias/</span>
				prefix is added automatically.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="kms-alias-name">Alias name</Label>
				<Input
					id="kms-alias-name"
					bind:value={aliasName}
					placeholder="my-app-key"
					autocomplete="off"
					class="font-mono text-xs"
				/>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="kms-alias-target">Target key</Label>
				<Select type="single" bind:value={targetKeyId}>
					<SelectTrigger id="kms-alias-target" class="w-full font-mono text-xs">
						{targetKeyId || 'Select a key'}
					</SelectTrigger>
					<SelectContent>
						{#each keys as k (k.keyId)}
							<SelectItem value={k.keyId} label={k.keyId}>{k.keyId}</SelectItem>
						{/each}
					</SelectContent>
				</Select>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !aliasName.trim() || !targetKeyId}>
				{creating ? 'Creating…' : 'Create alias'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
