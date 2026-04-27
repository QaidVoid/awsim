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
	import { createRepository } from '$lib/api/ecr';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: (name: string) => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let immutable = $state(false);
	let scanOnPush = $state(false);
	let creating = $state(false);

	function reset() {
		name = '';
		immutable = false;
		scanOnPush = false;
	}

	async function submit() {
		if (!name.trim()) {
			toast.error('Repository name is required.');
			return;
		}
		creating = true;
		try {
			const repo = await createRepository({
				repositoryName: name.trim(),
				imageTagMutability: immutable ? 'IMMUTABLE' : 'MUTABLE',
				scanOnPush,
			});
			toast.success('Repository created.');
			const created = repo.repositoryName;
			reset();
			onOpenChange(false);
			onCreated?.(created);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create repository');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New ECR repository</DialogTitle>
			<DialogDescription>
				Repositories store and version OCI / Docker container images.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="ecr-create-name">Repository name</Label>
				<Input
					id="ecr-create-name"
					bind:value={name}
					placeholder="my-app"
					autocomplete="off"
				/>
				<p class="text-[11px] text-muted-foreground">
					Lowercase alphanumeric, optionally separated by <code>/</code>, <code>_</code>, <code>-</code>.
				</p>
			</div>

			<div class="flex items-center justify-between rounded-md border border-border px-3 py-2">
				<div class="pr-3">
					<Label for="ecr-create-immutable" class="text-sm">Immutable tags</Label>
					<p class="text-[11px] text-muted-foreground">
						Once a tag is pushed it can't be overwritten.
					</p>
				</div>
				<Switch id="ecr-create-immutable" bind:checked={immutable} />
			</div>

			<div class="flex items-center justify-between rounded-md border border-border px-3 py-2">
				<div class="pr-3">
					<Label for="ecr-create-scan" class="text-sm">Scan on push</Label>
					<p class="text-[11px] text-muted-foreground">
						Run vulnerability scanning automatically on every push.
					</p>
				</div>
				<Switch id="ecr-create-scan" bind:checked={scanOnPush} />
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !name.trim()}>
				{creating ? 'Creating…' : 'Create repository'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
