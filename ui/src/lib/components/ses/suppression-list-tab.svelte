<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { DataTable, EmptyState } from '$lib/components/service';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import ShieldAlertIcon from '@lucide/svelte/icons/shield-alert';
	import { toast } from 'svelte-sonner';
	import {
		listSuppressedDestinations,
		putSuppressedDestination,
		deleteSuppressedDestination,
		type SuppressedDestination,
		type SuppressionReason
	} from '$lib/api/ses';

	let entries = $state<SuppressedDestination[]>([]);
	let loading = $state(false);
	let createOpen = $state(false);
	let newEmail = $state('');
	let newReason = $state<SuppressionReason>('BOUNCE');
	let creating = $state(false);
	let confirmDelete = $state<string | null>(null);

	async function load() {
		loading = true;
		try {
			entries = await listSuppressedDestinations();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load suppression list');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
	});

	async function add() {
		if (!newEmail.trim()) {
			toast.error('Email is required.');
			return;
		}
		creating = true;
		try {
			await putSuppressedDestination(newEmail.trim(), newReason);
			toast.success('Address suppressed.');
			newEmail = '';
			createOpen = false;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to add suppression');
		} finally {
			creating = false;
		}
	}

	async function remove(email: string) {
		try {
			await deleteSuppressedDestination(email);
			toast.success('Removed from suppression list.');
			confirmDelete = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to remove');
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">Account-level suppression list ({entries.length})</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={() => (createOpen = true)}>
				<PlusIcon /> Add
			</Button>
		</div>
	</div>

	{#snippet reasonCell(r: SuppressedDestination)}
		<Badge
			variant="outline"
			class={`h-4 px-1.5 text-[10px] ${
				r.reason === 'BOUNCE' ? 'text-destructive' : 'text-amber-500'
			}`}
		>
			{r.reason}
		</Badge>
	{/snippet}

	{#snippet actionsCell(r: SuppressedDestination)}
		<div class="flex justify-end">
			<Button
				size="xs"
				variant="ghost"
				class="text-destructive hover:text-destructive"
				aria-label="Remove suppression"
				onclick={() => (confirmDelete = r.emailAddress)}
			>
				<Trash2Icon />
			</Button>
		</div>
	{/snippet}

	<DataTable
		rows={entries}
		{loading}
		rowKey={(r) => r.emailAddress}
		columns={[
			{ key: 'emailAddress', label: 'Email', mono: true },
			{ key: 'reason', label: 'Reason', width: '120px', cell: reasonCell },
			{ key: 'lastUpdateTime', label: 'Last update', width: '230px' },
			{ key: 'actions', label: '', width: '60px', align: 'right', cell: actionsCell }
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={ShieldAlertIcon}
				title="No suppressed destinations"
				description="Bounces and complaints are auto-suppressed; you can also add entries manually."
			/>
		{/snippet}
	</DataTable>
</div>

<Dialog open={createOpen} onOpenChange={(o) => (createOpen = o)}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Add suppression</DialogTitle>
			<DialogDescription>
				Future sends to this email will be suppressed regardless of identity.
			</DialogDescription>
		</DialogHeader>
		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="ses-supp-email">Email</Label>
				<Input id="ses-supp-email" bind:value={newEmail} placeholder="bouncer@example.com" />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="ses-supp-reason">Reason</Label>
				<Select
					type="single"
					value={newReason}
					onValueChange={(v) => (newReason = v as SuppressionReason)}
				>
					<SelectTrigger id="ses-supp-reason" class="w-full text-sm">
						{newReason}
					</SelectTrigger>
					<SelectContent>
						<SelectItem value="BOUNCE" label="BOUNCE">BOUNCE</SelectItem>
						<SelectItem value="COMPLAINT" label="COMPLAINT">COMPLAINT</SelectItem>
					</SelectContent>
				</Select>
			</div>
		</div>
		<DialogFooter>
			<Button variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
			<Button onclick={add} disabled={creating || !newEmail.trim()}>
				{creating ? 'Adding…' : 'Add'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>

<Dialog
	open={confirmDelete !== null}
	onOpenChange={(o) => {
		if (!o) confirmDelete = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Remove from suppression list?</DialogTitle>
			<DialogDescription>
				<span class="font-mono">{confirmDelete}</span> will become deliverable again.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmDelete = null)}>Cancel</Button>
			<Button variant="destructive" onclick={() => confirmDelete && remove(confirmDelete)}>
				<Trash2Icon /> Remove
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
