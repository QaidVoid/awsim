<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { updateGroup, type CognitoGroup } from '$lib/api/cognito';
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
		group: CognitoGroup | null;
		onClose: () => void;
		onUpdated: () => void;
	}

	let { open = $bindable(false), poolId, group, onClose, onUpdated }: Props = $props();

	let description = $state('');
	let roleArn = $state('');
	let precedenceText = $state('');
	let saving = $state(false);
	let error = $state<string | null>(null);

	// Reset form to the group's current values whenever the dialog
	// opens or the target group changes. Without this the inputs would
	// keep stale state from a previous edit pass.
	$effect(() => {
		if (open && group) {
			description = group.description ?? '';
			roleArn = group.roleArn ?? '';
			precedenceText = group.precedence !== undefined ? String(group.precedence) : '';
			saving = false;
			error = null;
		}
	});

	async function submit() {
		if (!group) return;
		const precedence = precedenceText.trim() ? Number(precedenceText.trim()) : undefined;
		if (precedence !== undefined && Number.isNaN(precedence)) {
			error = 'Precedence must be a number';
			return;
		}
		saving = true;
		error = null;
		try {
			// AWS UpdateGroup uses absent fields as "leave unchanged" and
			// empty string to clear. Pass empty string for cleared values
			// so blanking the description or role-arn actually removes
			// them rather than getting silently dropped.
			await updateGroup({
				poolId,
				name: group.name,
				description: description.trim(),
				roleArn: roleArn.trim(),
				precedence
			});
			toast.success(`Updated group ${group.name}`);
			onUpdated();
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Update group failed';
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
			<DialogTitle>Edit group {group?.name ?? ''}</DialogTitle>
			<DialogDescription>Group name is immutable; everything else can change.</DialogDescription>
		</DialogHeader>
		<form
			class="flex flex-col gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			<div class="flex flex-col gap-1.5">
				<Label for="edit-grp-desc">Description</Label>
				<Input id="edit-grp-desc" bind:value={description} autocomplete="off" />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="edit-grp-role">Role ARN</Label>
				<RoleArnPicker id="edit-grp-role" bind:value={roleArn} />
				<p class="text-[11px] text-muted-foreground">
					Empty clears the role binding for this group.
				</p>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="edit-grp-prec">Precedence (lower = higher priority)</Label>
				<Input
					id="edit-grp-prec"
					type="number"
					bind:value={precedenceText}
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
				<Button type="submit" disabled={saving}>
					{#if saving}
						<Loader2 class="size-3.5 animate-spin" />
					{/if}
					Save
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
