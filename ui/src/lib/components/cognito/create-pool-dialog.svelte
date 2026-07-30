<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { createUserPool } from '$lib/api/cognito';
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
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		open: boolean;
		onClose: () => void;
		onCreated: () => void;
	}

	let { open = $bindable(false), onClose, onCreated }: Props = $props();

	let name = $state('');
	let autoVerifyEmail = $state(true);
	let autoVerifyPhone = $state(false);
	let signInWithEmail = $state(false);
	let minLength = $state(8);
	// Cognito's own defaults, so a pool made here matches one made with
	// the CLI and no explicit policy.
	let requireUppercase = $state(true);
	let requireLowercase = $state(true);
	let requireNumbers = $state(true);
	let requireSymbols = $state(true);
	let tempPasswordDays = $state(7);
	let mfaConfiguration = $state<'OFF' | 'OPTIONAL' | 'ON'>('OFF');
	let tagsText = $state('');
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!open) {
			name = '';
			autoVerifyEmail = true;
			autoVerifyPhone = false;
			signInWithEmail = false;
			minLength = 8;
			requireUppercase = true;
			requireLowercase = true;
			requireNumbers = true;
			requireSymbols = true;
			tempPasswordDays = 7;
			mfaConfiguration = 'OFF';
			tagsText = '';
			saving = false;
			error = null;
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
		if (!name.trim()) {
			error = 'Pool name is required';
			return;
		}
		const minLen = Number(minLength);
		if (Number.isNaN(minLen) || minLen < 6 || minLen > 99) {
			error = 'Minimum password length must be between 6 and 99';
			return;
		}
		const tempDays = Number(tempPasswordDays);
		if (Number.isNaN(tempDays) || tempDays < 0 || tempDays > 365) {
			error = 'Temporary password validity must be between 0 and 365 days';
			return;
		}
		const tags = parseTags();
		if (tags === null) {
			error = 'Tags must be one `key=value` per line';
			return;
		}
		const autoVerified = [
			...(autoVerifyEmail ? ['email'] : []),
			...(autoVerifyPhone ? ['phone_number'] : [])
		];
		saving = true;
		error = null;
		try {
			await createUserPool({
				name: name.trim(),
				autoVerifiedAttributes: autoVerified,
				usernameAttributes: signInWithEmail ? ['email'] : [],
				passwordMinLength: minLen,
				requireUppercase,
				requireLowercase,
				requireNumbers,
				requireSymbols,
				temporaryPasswordValidityDays: tempDays,
				mfaConfiguration,
				tags
			});
			toast.success(`Created ${name.trim()}`);
			onCreated();
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Create pool failed';
			error = msg;
			toast.error(msg);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<DialogContent class="sm:max-w-md max-h-[85vh] overflow-y-auto">
		<DialogHeader>
			<DialogTitle>Create user pool</DialogTitle>
			<DialogDescription>You can tune attributes + flows after creation.</DialogDescription>
		</DialogHeader>
		<form
			class="flex flex-col gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			<div class="flex flex-col gap-1.5">
				<Label for="pool-name">Pool name</Label>
				<Input id="pool-name" bind:value={name} placeholder="my-app-users" autocomplete="off" />
			</div>
			<p class="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
				Sign-in
			</p>
			<label class="flex items-center gap-2 text-xs text-muted-foreground">
				<input type="checkbox" bind:checked={signInWithEmail} class="size-3.5" />
				Allow sign-in with an email address instead of a username
			</label>
			<label class="flex items-center gap-2 text-xs text-muted-foreground">
				<input type="checkbox" bind:checked={autoVerifyEmail} class="size-3.5" />
				Auto-verify email attribute
			</label>
			<label class="flex items-center gap-2 text-xs text-muted-foreground">
				<input type="checkbox" bind:checked={autoVerifyPhone} class="size-3.5" />
				Auto-verify phone number attribute
			</label>
			<div class="flex flex-col gap-1.5">
				<Label for="pool-mfa">Multi-factor authentication</Label>
				<select
					id="pool-mfa"
					bind:value={mfaConfiguration}
					class="h-9 rounded-md border border-border bg-background px-2 text-sm"
				>
					<option value="OFF">Off</option>
					<option value="OPTIONAL">Optional</option>
					<option value="ON">Required</option>
				</select>
			</div>

			<p class="pt-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
				Password policy
			</p>
			<div class="grid grid-cols-2 gap-3">
				<div class="flex flex-col gap-1.5">
					<Label for="pool-min">Minimum length</Label>
					<Input
						id="pool-min"
						type="number"
						bind:value={minLength}
						min="6"
						max="99"
						autocomplete="off"
					/>
				</div>
				<div class="flex flex-col gap-1.5">
					<Label for="pool-temp-days">Temporary password (days)</Label>
					<Input
						id="pool-temp-days"
						type="number"
						bind:value={tempPasswordDays}
						min="0"
						max="365"
						autocomplete="off"
					/>
				</div>
			</div>
			<div class="grid grid-cols-2 gap-1">
				<label class="flex items-center gap-2 text-xs text-muted-foreground">
					<input type="checkbox" bind:checked={requireUppercase} class="size-3.5" />
					Require uppercase
				</label>
				<label class="flex items-center gap-2 text-xs text-muted-foreground">
					<input type="checkbox" bind:checked={requireLowercase} class="size-3.5" />
					Require lowercase
				</label>
				<label class="flex items-center gap-2 text-xs text-muted-foreground">
					<input type="checkbox" bind:checked={requireNumbers} class="size-3.5" />
					Require numbers
				</label>
				<label class="flex items-center gap-2 text-xs text-muted-foreground">
					<input type="checkbox" bind:checked={requireSymbols} class="size-3.5" />
					Require symbols
				</label>
			</div>

			<div class="flex flex-col gap-1.5">
				<Label for="pool-tags">Tags</Label>
				<textarea
					id="pool-tags"
					bind:value={tagsText}
					rows="2"
					placeholder="env=dev&#10;team=platform"
					class="rounded-md border border-border bg-background px-2 py-1.5 font-mono text-xs"
				></textarea>
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
