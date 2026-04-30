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
	let minLengthText = $state('8');
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!open) {
			name = '';
			autoVerifyEmail = true;
			minLengthText = '8';
			saving = false;
			error = null;
		}
	});

	async function submit() {
		if (!name.trim()) {
			error = 'Pool name is required';
			return;
		}
		const minLen = Number(minLengthText.trim());
		if (Number.isNaN(minLen) || minLen < 6) {
			error = 'Minimum password length must be ≥ 6';
			return;
		}
		saving = true;
		error = null;
		try {
			await createUserPool({
				name: name.trim(),
				autoVerifyEmail,
				passwordMinLength: minLen
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
	<DialogContent class="sm:max-w-md">
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
			<label class="flex items-center gap-2 text-xs text-muted-foreground">
				<input type="checkbox" bind:checked={autoVerifyEmail} class="size-3.5" />
				Auto-verify email attribute
			</label>
			<div class="flex flex-col gap-1.5">
				<Label for="pool-min">Password minimum length</Label>
				<Input
					id="pool-min"
					type="number"
					bind:value={minLengthText}
					min="6"
					autocomplete="off"
				/>
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
