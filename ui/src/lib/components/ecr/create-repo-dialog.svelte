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
	import { validateEcrRepositoryName } from '$lib/validators';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: (name: string) => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let immutable = $state(false);
	let scanOnPush = $state(false);
	let encryptionType = $state<'AES256' | 'KMS'>('AES256');
	let kmsKey = $state('');
	let tagsText = $state('');
	let creating = $state(false);

	const nameError = $derived(
		name.trim() ? validateEcrRepositoryName(name.trim()) : null
	);

	function reset() {
		name = '';
		immutable = false;
		scanOnPush = false;
		encryptionType = 'AES256';
		kmsKey = '';
		tagsText = '';
	}

	/** Parse `key=value` lines into a tag map, rejecting malformed rows. */
	function parseTags(): Record<string, string> | null {
		const tags: Record<string, string> = {};
		for (const line of tagsText.split('\n')) {
			const trimmed = line.trim();
			if (!trimmed) continue;
			const eq = trimmed.indexOf('=');
			if (eq <= 0) return null;
			tags[trimmed.slice(0, eq).trim()] = trimmed.slice(eq + 1).trim();
		}
		return tags;
	}

	async function submit() {
		if (!name.trim()) {
			toast.error('Repository name is required.');
			return;
		}
		if (nameError) {
			toast.error(nameError);
			return;
		}
		if (encryptionType === 'KMS' && !kmsKey.trim()) {
			toast.error('KMS encryption needs a key id, ARN, or alias.');
			return;
		}
		const tags = parseTags();
		if (tags === null) {
			toast.error('Tags must be one `key=value` per line.');
			return;
		}
		creating = true;
		try {
			const repo = await createRepository({
				repositoryName: name.trim(),
				imageTagMutability: immutable ? 'IMMUTABLE' : 'MUTABLE',
				scanOnPush,
				encryptionType,
				kmsKey: kmsKey.trim() || undefined,
				tags,
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
	<DialogContent class="sm:max-w-md max-h-[85vh] overflow-y-auto">
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
					aria-invalid={nameError ? 'true' : undefined}
				/>
				{#if nameError}
					<p class="text-[11px] text-destructive">{nameError}</p>
				{:else}
					<p class="text-[11px] text-muted-foreground">
						Lowercase alphanumeric, optionally separated by <code>/</code>, <code>_</code>, <code>-</code>.
					</p>
				{/if}
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

			<div class="flex flex-col gap-1">
				<Label for="ecr-create-encryption">Encryption</Label>
				<select
					id="ecr-create-encryption"
					bind:value={encryptionType}
					class="h-9 rounded-md border border-border bg-background px-2 text-sm"
				>
					<option value="AES256">AES-256 (ECR managed)</option>
					<option value="KMS">KMS</option>
				</select>
			</div>

			{#if encryptionType === 'KMS'}
				<div class="flex flex-col gap-1">
					<Label for="ecr-create-kms">KMS key</Label>
					<Input
						id="ecr-create-kms"
						bind:value={kmsKey}
						placeholder="alias/aws/ecr"
						autocomplete="off"
					/>
					<p class="text-[11px] text-muted-foreground">
						Key id, ARN, or alias. Required for KMS encryption.
					</p>
				</div>
			{/if}

			<div class="flex flex-col gap-1">
				<Label for="ecr-create-tags">Tags</Label>
				<textarea
					id="ecr-create-tags"
					bind:value={tagsText}
					rows="2"
					placeholder="env=dev&#10;team=platform"
					class="rounded-md border border-border bg-background px-2 py-1.5 font-mono text-xs"
				></textarea>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button
				onclick={submit}
				disabled={creating || !name.trim() || nameError !== null}
			>
				{creating ? 'Creating...' : 'Create repository'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
