<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { createAppClient } from '$lib/api/cognito';
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
		onCreated: (clientId: string) => void;
	}

	let { open = $bindable(false), poolId, onClose, onCreated }: Props = $props();

	let clientName = $state('');
	let generateSecret = $state(false);
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!open) {
			clientName = '';
			generateSecret = false;
			saving = false;
			error = null;
		}
	});

	async function submit() {
		if (!clientName.trim()) {
			error = 'Client name is required';
			return;
		}
		saving = true;
		error = null;
		try {
			const c = await createAppClient({
				poolId,
				clientName: clientName.trim(),
				generateSecret,
				explicitAuthFlows: ['ALLOW_USER_PASSWORD_AUTH', 'ALLOW_REFRESH_TOKEN_AUTH']
			});
			toast.success(`Created ${clientName.trim()}`);
			onCreated(c.clientId);
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Create client failed';
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
			<DialogTitle>Create app client</DialogTitle>
			<DialogDescription>Edit OAuth + callback URLs after creation.</DialogDescription>
		</DialogHeader>
		<form
			class="flex flex-col gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			<div class="flex flex-col gap-1.5">
				<Label for="cli-name">Name</Label>
				<Input id="cli-name" bind:value={clientName} placeholder="web-app" autocomplete="off" />
			</div>
			<label class="flex items-center gap-2 text-xs text-muted-foreground">
				<input type="checkbox" bind:checked={generateSecret} class="size-3.5" />
				Generate client secret (confidential client)
			</label>
			{#if error}
				<p class="text-xs text-destructive">{error}</p>
			{/if}
			<DialogFooter>
				<Button type="button" variant="outline" onclick={onClose} disabled={saving}>
					Cancel
				</Button>
				<Button type="submit" disabled={saving || !clientName.trim()}>
					{#if saving}
						<Loader2 class="size-3.5 animate-spin" />
					{/if}
					Create
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
