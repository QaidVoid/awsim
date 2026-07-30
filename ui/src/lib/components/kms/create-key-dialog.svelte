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

	// Asymmetric specs, grouped by what they can be used for. KMS ties
	// the two together: an ECC key cannot encrypt, so offering every
	// combination would only let you build a request KMS rejects.
	const ENCRYPT_SPECS = ['RSA_2048', 'RSA_3072', 'RSA_4096'];
	const SIGN_SPECS = [
		'RSA_2048',
		'RSA_3072',
		'RSA_4096',
		'ECC_NIST_P256',
		'ECC_NIST_P384',
		'ECC_NIST_P521',
	];

	let symmetric = $state(true);
	let keyUsage = $state<'ENCRYPT_DECRYPT' | 'SIGN_VERIFY'>('ENCRYPT_DECRYPT');
	let keySpec = $state('RSA_2048');
	let origin = $state<'AWS_KMS' | 'EXTERNAL'>('AWS_KMS');
	let description = $state('');
	let alias = $state('');
	let tagsText = $state('');
	let creating = $state(false);

	let specChoices = $derived(keyUsage === 'SIGN_VERIFY' ? SIGN_SPECS : ENCRYPT_SPECS);

	// Switching usage can strand a spec that no longer applies, so pull
	// it back to the first valid one.
	$effect(() => {
		if (!specChoices.includes(keySpec)) keySpec = specChoices[0];
	});

	function reset() {
		symmetric = true;
		keyUsage = 'ENCRYPT_DECRYPT';
		keySpec = 'RSA_2048';
		origin = 'AWS_KMS';
		description = '';
		alias = '';
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
		const tags = parseTags();
		if (tags === null) {
			toast.error('Tags must be one `key=value` per line.');
			return;
		}
		creating = true;
		try {
			const k = await createKey({
				description: description.trim() || undefined,
				keySpec: symmetric ? 'SYMMETRIC_DEFAULT' : keySpec,
				keyUsage: symmetric ? 'ENCRYPT_DECRYPT' : keyUsage,
				origin,
				alias: alias.trim() || undefined,
				tags,
			});
			toast.success('Key created.');
			reset();
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
	<DialogContent class="sm:max-w-md max-h-[85vh] overflow-y-auto">
		<DialogHeader>
			<DialogTitle>New KMS key</DialogTitle>
			<DialogDescription>
				A customer-managed key. Give it an alias to refer to it by name instead of id.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="kms-create-type">Key type</Label>
				<select
					id="kms-create-type"
					value={symmetric ? 'symmetric' : 'asymmetric'}
					onchange={(e) => (symmetric = e.currentTarget.value === 'symmetric')}
					class="h-9 rounded-md border border-border bg-background px-2 text-sm"
				>
					<option value="symmetric">Symmetric</option>
					<option value="asymmetric">Asymmetric</option>
				</select>
				<p class="text-[11px] text-muted-foreground">
					{symmetric
						? 'One key both encrypts and decrypts.'
						: 'A public/private pair. The private half never leaves KMS.'}
				</p>
			</div>

			{#if !symmetric}
				<div class="flex flex-col gap-1">
					<Label for="kms-create-usage">Key usage</Label>
					<select
						id="kms-create-usage"
						bind:value={keyUsage}
						class="h-9 rounded-md border border-border bg-background px-2 text-sm"
					>
						<option value="ENCRYPT_DECRYPT">Encrypt and decrypt</option>
						<option value="SIGN_VERIFY">Sign and verify</option>
					</select>
				</div>

				<div class="flex flex-col gap-1">
					<Label for="kms-create-spec">Key spec</Label>
					<select
						id="kms-create-spec"
						bind:value={keySpec}
						class="h-9 rounded-md border border-border bg-background px-2 text-sm"
					>
						{#each specChoices as spec (spec)}
							<option value={spec}>{spec}</option>
						{/each}
					</select>
				</div>
			{/if}

			<div class="flex flex-col gap-1">
				<Label for="kms-create-origin">Key material origin</Label>
				<select
					id="kms-create-origin"
					bind:value={origin}
					class="h-9 rounded-md border border-border bg-background px-2 text-sm"
				>
					<option value="AWS_KMS">KMS (generated for you)</option>
					<option value="EXTERNAL">External (you import it)</option>
				</select>
				{#if origin === 'EXTERNAL'}
					<p class="text-[11px] text-muted-foreground">
						The key starts in `PendingImport` and cannot be used until you run
						GetParametersForImport and ImportKeyMaterial.
					</p>
				{/if}
			</div>

			<div class="flex flex-col gap-1">
				<Label for="kms-create-alias">Alias (optional)</Label>
				<Input id="kms-create-alias" bind:value={alias} placeholder="app-data" />
				<p class="text-[11px] text-muted-foreground">
					`alias/` is added automatically.
				</p>
			</div>

			<div class="flex flex-col gap-1">
				<Label for="kms-create-desc">Description (optional)</Label>
				<Input
					id="kms-create-desc"
					bind:value={description}
					placeholder="App data encryption key"
				/>
			</div>

			<div class="flex flex-col gap-1">
				<Label for="kms-create-tags">Tags</Label>
				<textarea
					id="kms-create-tags"
					bind:value={tagsText}
					rows="2"
					placeholder="env=dev&#10;team=platform"
					class="rounded-md border border-border bg-background px-2 py-1.5 text-sm font-mono"
				></textarea>
				<p class="text-[11px] text-muted-foreground">One `key=value` per line.</p>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating}>
				{creating ? 'Creating...' : 'Create key'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
