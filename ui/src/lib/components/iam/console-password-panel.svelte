<script lang="ts">
	import { onMount } from 'svelte';
	import {
		getLoginProfile,
		createLoginProfile,
		updateLoginProfile,
		deleteLoginProfile,
		type IamLoginProfile
	} from '$lib/api/iam';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import { Switch } from '$lib/components/ui/switch';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { toast } from 'svelte-sonner';

	interface Props {
		userName: string;
	}

	let { userName }: Props = $props();

	let profile = $state<IamLoginProfile | null>(null);
	let loading = $state(false);
	let saving = $state(false);
	let password = $state('');
	let confirmPassword = $state('');
	let resetRequired = $state(false);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	const hasProfile = $derived(profile !== null);
	const passwordsMatch = $derived(
		password.length === 0 || password === confirmPassword
	);
	const canSubmit = $derived(
		password.length >= 8 && passwordsMatch && !saving
	);

	onMount(load);

	$effect(() => {
		void userName;
		load();
	});

	async function load() {
		if (!userName) return;
		loading = true;
		profile = null;
		password = '';
		confirmPassword = '';
		resetRequired = false;
		try {
			profile = await getLoginProfile(userName);
			if (profile) {
				resetRequired = profile.passwordResetRequired;
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load login profile');
		} finally {
			loading = false;
		}
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!canSubmit) return;
		saving = true;
		try {
			if (hasProfile) {
				await updateLoginProfile(userName, {
					password,
					passwordResetRequired: resetRequired
				});
				toast.success('Password updated.');
			} else {
				await createLoginProfile(userName, password, resetRequired);
				toast.success('Console password enabled.');
			}
			password = '';
			confirmPassword = '';
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Save failed');
		} finally {
			saving = false;
		}
	}

	async function toggleResetOnly() {
		if (!profile) return;
		saving = true;
		try {
			await updateLoginProfile(userName, {
				passwordResetRequired: resetRequired
			});
			toast.success(
				resetRequired
					? 'User will be required to change password on next sign-in.'
					: 'Forced password reset cleared.'
			);
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		} finally {
			saving = false;
		}
	}

	async function confirmDelete() {
		deleteBusy = true;
		try {
			await deleteLoginProfile(userName);
			toast.success('Console password removed.');
			deleteOpen = false;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}

	function formatDate(s: string): string {
		try {
			return new Date(s).toLocaleString();
		} catch {
			return s;
		}
	}
</script>

<section>
	<div class="mb-2 flex items-center justify-between">
		<h3 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
			Console password
		</h3>
		{#if hasProfile}
			<Badge variant="default">Enabled</Badge>
		{:else if !loading}
			<Badge variant="secondary">Not set</Badge>
		{/if}
	</div>

	{#if loading}
		<p class="text-xs text-muted-foreground">Loading...</p>
	{:else}
		{#if hasProfile && profile}
			<dl class="mb-3 grid grid-cols-3 gap-x-4 gap-y-1 text-xs">
				<dt class="text-muted-foreground">Created</dt>
				<dd class="col-span-2">{formatDate(profile.createDate)}</dd>
				<dt class="text-muted-foreground">Reset required</dt>
				<dd class="col-span-2">
					{profile.passwordResetRequired ? 'Yes' : 'No'}
				</dd>
			</dl>
		{/if}

		<form onsubmit={submit} class="flex flex-col gap-2.5 rounded-md border border-border/60 p-3">
			<div class="flex flex-col gap-1">
				<Label for="console-pw" class="text-xs">
					{hasProfile ? 'New password' : 'Password'}
				</Label>
				<Input
					id="console-pw"
					type="password"
					bind:value={password}
					minlength={8}
					autocomplete="new-password"
					placeholder="Minimum 8 characters"
				/>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="console-pw-confirm" class="text-xs">Confirm</Label>
				<Input
					id="console-pw-confirm"
					type="password"
					bind:value={confirmPassword}
					minlength={8}
					autocomplete="new-password"
					aria-invalid={!passwordsMatch ? 'true' : undefined}
				/>
				{#if !passwordsMatch}
					<p class="text-[11px] text-destructive">Passwords do not match.</p>
				{/if}
			</div>
			<div class="flex items-center justify-between rounded-md border border-border/60 px-3 py-2">
				<div class="pr-3">
					<Label class="text-xs" for="console-pw-reset">
						Require password reset on next sign-in
					</Label>
					<p class="text-[11px] text-muted-foreground">
						Forces the user to rotate the password before they can use the console.
					</p>
				</div>
				<Switch id="console-pw-reset" bind:checked={resetRequired} />
			</div>

			<div class="mt-1 flex items-center justify-between gap-2">
				{#if hasProfile}
					<Button
						type="button"
						variant="ghost"
						size="sm"
						class="text-destructive hover:bg-destructive/10"
						onclick={() => (deleteOpen = true)}
					>
						<Trash2 class="size-3.5" />
						Remove password
					</Button>
					<div class="flex items-center gap-2">
						{#if password.length === 0 && resetRequired !== profile?.passwordResetRequired}
							<Button
								type="button"
								variant="outline"
								size="sm"
								onclick={toggleResetOnly}
								disabled={saving}
							>
								{#if saving}<Loader2 class="size-3.5 animate-spin" />{/if}
								Apply reset toggle
							</Button>
						{/if}
						<Button type="submit" size="sm" disabled={!canSubmit}>
							{#if saving}
								<Loader2 class="size-3.5 animate-spin" />
							{:else}
								<KeyRound class="size-3.5" />
							{/if}
							Change password
						</Button>
					</div>
				{:else}
					<span></span>
					<Button type="submit" size="sm" disabled={!canSubmit}>
						{#if saving}
							<Loader2 class="size-3.5 animate-spin" />
						{:else}
							<KeyRound class="size-3.5" />
						{/if}
						Enable console password
					</Button>
				{/if}
			</div>
		</form>
	{/if}
</section>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Remove console password?"
	description={`Delete the login profile for ${userName}. They will no longer be able to sign in to the console until a new password is set.`}
	confirmLabel="Remove"
	busy={deleteBusy}
	onConfirm={confirmDelete}
	onClose={() => (deleteOpen = false)}
/>
