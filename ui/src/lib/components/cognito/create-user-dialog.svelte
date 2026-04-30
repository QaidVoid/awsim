<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { adminCreateUser } from '$lib/api/cognito';
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
		poolId: string;
		onClose: () => void;
		onCreated: () => void;
	}

	let { open = $bindable(false), poolId, onClose, onCreated }: Props = $props();

	let username = $state('');
	let temporaryPassword = $state('');
	let email = $state('');
	let phone = $state('');
	let suppressInvite = $state(true);
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!open) {
			username = '';
			temporaryPassword = '';
			email = '';
			phone = '';
			suppressInvite = true;
			saving = false;
			error = null;
		}
	});

	async function submit() {
		if (!username.trim()) {
			error = 'Username is required';
			return;
		}
		saving = true;
		error = null;
		const attrs: { name: string; value: string }[] = [];
		if (email.trim()) attrs.push({ name: 'email', value: email.trim() });
		if (phone.trim()) attrs.push({ name: 'phone_number', value: phone.trim() });
		try {
			await adminCreateUser({
				poolId,
				username: username.trim(),
				temporaryPassword: temporaryPassword.trim() || undefined,
				attributes: attrs.length > 0 ? attrs : undefined,
				messageAction: suppressInvite ? 'SUPPRESS' : undefined
			});
			toast.success(`Created ${username.trim()}`);
			onCreated();
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Create user failed';
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
			<DialogTitle>Create user</DialogTitle>
			<DialogDescription>Admin-created users start in FORCE_CHANGE_PASSWORD.</DialogDescription>
		</DialogHeader>
		<form
			class="flex flex-col gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			<div class="flex flex-col gap-1.5">
				<Label for="user-name">Username</Label>
				<Input id="user-name" bind:value={username} placeholder="alice" autocomplete="off" />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="user-pw">Temporary password (optional)</Label>
				<Input
					id="user-pw"
					type="text"
					bind:value={temporaryPassword}
					placeholder="leave blank to auto-generate"
					autocomplete="off"
				/>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="user-email">Email</Label>
				<Input
					id="user-email"
					type="email"
					bind:value={email}
					placeholder="alice@example.com"
					autocomplete="off"
				/>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="user-phone">Phone</Label>
				<Input
					id="user-phone"
					bind:value={phone}
					placeholder="+15551234567"
					autocomplete="off"
				/>
			</div>
			<label class="flex items-center gap-2 text-xs text-muted-foreground">
				<input type="checkbox" bind:checked={suppressInvite} class="size-3.5" />
				Suppress invitation message (MessageAction=SUPPRESS)
			</label>
			{#if error}
				<p class="text-xs text-destructive">{error}</p>
			{/if}
			<DialogFooter>
				<Button type="button" variant="outline" onclick={onClose} disabled={saving}>
					Cancel
				</Button>
				<Button type="submit" disabled={saving || !username.trim()}>
					{#if saving}
						<Loader2 class="size-3.5 animate-spin" />
					{/if}
					Create
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
