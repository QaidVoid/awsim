<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { adminSetUserPassword } from '$lib/api/cognito';
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
		username: string;
		onClose: () => void;
	}

	let { open = $bindable(false), poolId, username, onClose }: Props = $props();

	let password = $state('');
	let permanent = $state(true);
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!open) {
			password = '';
			permanent = true;
			saving = false;
			error = null;
		}
	});

	async function submit() {
		if (!password) {
			error = 'Password is required';
			return;
		}
		saving = true;
		error = null;
		try {
			await adminSetUserPassword({ poolId, username, password, permanent });
			toast.success(`Password set for ${username}`);
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Set password failed';
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
			<DialogTitle>Set password</DialogTitle>
			<DialogDescription class="font-mono text-xs">{username}</DialogDescription>
		</DialogHeader>
		<form
			class="flex flex-col gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			<div class="flex flex-col gap-1.5">
				<Label for="set-pw">New password</Label>
				<Input id="set-pw" type="text" bind:value={password} autocomplete="off" autofocus />
			</div>
			<label class="flex items-center gap-2 text-xs text-muted-foreground">
				<input type="checkbox" bind:checked={permanent} class="size-3.5" />
				Permanent (otherwise user must change at next sign-in)
			</label>
			{#if error}
				<p class="text-xs text-destructive">{error}</p>
			{/if}
			<DialogFooter>
				<Button type="button" variant="outline" onclick={onClose} disabled={saving}>
					Cancel
				</Button>
				<Button type="submit" disabled={saving || !password}>
					{#if saving}
						<Loader2 class="size-3.5 animate-spin" />
					{/if}
					Set
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
