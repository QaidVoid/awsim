<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { createGroup } from '$lib/api/cognito';
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
	import RoleArnPicker from './role-arn-picker.svelte';

	interface Props {
		open: boolean;
		poolId: string;
		onClose: () => void;
		onCreated: () => void;
	}

	let { open = $bindable(false), poolId, onClose, onCreated }: Props = $props();

	let name = $state('');
	let description = $state('');
	let roleArn = $state('');
	let precedenceInput = $state<number | string>('');
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!open) {
			name = '';
			description = '';
			roleArn = '';
			precedenceInput = '';
			saving = false;
			error = null;
		}
	});

	async function submit() {
		if (!name.trim()) {
			error = 'Group name is required';
			return;
		}
		const precedence = precedenceInput === '' || precedenceInput === null ? undefined : Number(precedenceInput);
		if (precedence !== undefined && Number.isNaN(precedence)) {
			error = 'Precedence must be a number';
			return;
		}
		saving = true;
		error = null;
		try {
			await createGroup({
				poolId,
				name: name.trim(),
				description: description.trim() || undefined,
				roleArn: roleArn.trim() || undefined,
				precedence
			});
			toast.success(`Created group ${name.trim()}`);
			onCreated();
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Create group failed';
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
			<DialogTitle>Create group</DialogTitle>
			<DialogDescription>Groups become claims in the user's tokens.</DialogDescription>
		</DialogHeader>
		<form
			class="flex flex-col gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			<div class="flex flex-col gap-1.5">
				<Label for="grp-name">Name</Label>
				<Input id="grp-name" bind:value={name} placeholder="admins" autocomplete="off" />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="grp-desc">Description</Label>
				<Input id="grp-desc" bind:value={description} autocomplete="off" />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="grp-role">Role ARN (optional)</Label>
				<RoleArnPicker id="grp-role" bind:value={roleArn} />
				<p class="text-[11px] text-muted-foreground">
					Surfaces as <code class="font-mono">cognito:preferred_role</code> in the user's
					ID token; Identity Pool Token-type role mapping vends temp creds for this role.
				</p>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="grp-prec">Precedence (lower = higher priority)</Label>
				<Input
					id="grp-prec"
					type="number"
					bind:value={precedenceInput}
					placeholder="optional"
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
