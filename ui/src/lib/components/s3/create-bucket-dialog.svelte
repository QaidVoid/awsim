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
	import { Switch } from '$lib/components/ui/switch';
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
	let versioning = $state(false);
	let objectLock = $state(false);
	let blockPublicAccess = $state(true);
	let encryption = $state<'none' | 'AES256' | 'aws:kms'>('none');
	let kmsKeyId = $state('');
	let tagsText = $state('');
	let saving = $state(false);
	let error = $state<string | null>(null);

	// Object Lock forces versioning on and there is no way to turn it off
	// afterwards, so reflect that in the toggle rather than letting the
	// two controls disagree.
	let versioningOn = $derived(versioning || objectLock);

	$effect(() => {
		if (!open) {
			name = '';
			versioning = false;
			objectLock = false;
			blockPublicAccess = true;
			encryption = 'none';
			kmsKeyId = '';
			tagsText = '';
			error = null;
			saving = false;
		}
	});

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
		const trimmed = name.trim();
		if (!trimmed) {
			error = 'Bucket name is required';
			return;
		}
		const tags = parseTags();
		if (tags === null) {
			error = 'Tags must be one `key=value` per line';
			return;
		}
		if (encryption === 'aws:kms' && !kmsKeyId.trim()) {
			error = 'SSE-KMS needs a key id or ARN';
			return;
		}
		saving = true;
		error = null;
		try {
			await createBucket(trimmed, {
				versioning: versioningOn,
				objectLock,
				blockPublicAccess,
				encryption: encryption === 'none' ? undefined : encryption,
				kmsKeyId: kmsKeyId.trim() || undefined,
				tags
			});
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
	<DialogContent class="sm:max-w-lg max-h-[85vh] overflow-y-auto">
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

			<div class="flex items-center justify-between rounded-md border border-border px-3 py-2">
				<div>
					<Label for="new-bucket-bpa" class="text-sm">Block public access</Label>
					<p class="text-[11px] text-muted-foreground">Refuse public ACLs and policies.</p>
				</div>
				<Switch id="new-bucket-bpa" bind:checked={blockPublicAccess} />
			</div>

			<div class="flex items-center justify-between rounded-md border border-border px-3 py-2">
				<div>
					<Label for="new-bucket-versioning" class="text-sm">Versioning</Label>
					<p class="text-[11px] text-muted-foreground">
						{objectLock ? 'Required by Object Lock.' : 'Keep every version of an object.'}
					</p>
				</div>
				<Switch
					id="new-bucket-versioning"
					checked={versioningOn}
					disabled={objectLock}
					onCheckedChange={(v: boolean) => (versioning = v)}
				/>
			</div>

			<div class="flex items-center justify-between rounded-md border border-border px-3 py-2">
				<div>
					<Label for="new-bucket-lock" class="text-sm">Object Lock</Label>
					<p class="text-[11px] text-muted-foreground">
						Create-time only. It cannot be enabled later.
					</p>
				</div>
				<Switch id="new-bucket-lock" bind:checked={objectLock} />
			</div>

			<div class="flex flex-col gap-1.5">
				<Label for="new-bucket-enc">Default encryption</Label>
				<select
					id="new-bucket-enc"
					bind:value={encryption}
					class="h-9 rounded-md border border-border bg-background px-2 text-sm"
				>
					<option value="none">None</option>
					<option value="AES256">SSE-S3 (AES256)</option>
					<option value="aws:kms">SSE-KMS</option>
				</select>
			</div>

			{#if encryption === 'aws:kms'}
				<div class="flex flex-col gap-1.5">
					<Label for="new-bucket-kms">KMS key id or ARN</Label>
					<Input
						id="new-bucket-kms"
						bind:value={kmsKeyId}
						placeholder="alias/aws/s3"
						autocomplete="off"
					/>
				</div>
			{/if}

			<div class="flex flex-col gap-1.5">
				<Label for="new-bucket-tags">Tags</Label>
				<textarea
					id="new-bucket-tags"
					bind:value={tagsText}
					rows="3"
					placeholder="env=dev&#10;team=platform"
					class="rounded-md border border-border bg-background px-2 py-1.5 text-sm font-mono"
				></textarea>
				<p class="text-[11px] text-muted-foreground">One `key=value` per line.</p>
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
