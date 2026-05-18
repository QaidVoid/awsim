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
	import { createAccount } from '$lib/api/organizations';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let accountName = $state('');
	let email = $state('');
	let creating = $state(false);

	async function submit() {
		if (!accountName.trim()) {
			toast.error('Account name is required.');
			return;
		}
		if (!email.trim()) {
			toast.error('Email is required.');
			return;
		}
		creating = true;
		try {
			const r = await createAccount(accountName.trim(), email.trim());
			toast.success(
				r.state ? `Account creation ${r.state.toLowerCase()}.` : 'Account created.'
			);
			accountName = '';
			email = '';
			onOpenChange(false);
			onCreated?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create account');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New member account</DialogTitle>
			<DialogDescription>
				Creates a member account in the organization with a unique email.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="acct-name">Account name</Label>
				<Input
					id="acct-name"
					bind:value={accountName}
					placeholder="staging"
					autocomplete="off"
				/>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="acct-email">Email</Label>
				<Input
					id="acct-email"
					bind:value={email}
					placeholder="aws+staging@example.com"
					autocomplete="off"
					class="font-mono text-xs"
				/>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !accountName.trim() || !email.trim()}>
				{creating ? 'Creating…' : 'Create account'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
