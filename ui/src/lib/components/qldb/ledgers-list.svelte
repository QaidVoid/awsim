<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import LockOpenIcon from '@lucide/svelte/icons/lock-open';
	import BookTextIcon from '@lucide/svelte/icons/book-text';
	import { toast } from 'svelte-sonner';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import {
		listLedgers,
		describeLedger,
		createLedger,
		deleteLedger,
		updateLedgerProtection,
		type LedgerSummary
	} from '$lib/api/qldb';

	let rows = $state<LedgerSummary[]>([]);
	let loading = $state(false);
	let newName = $state('');
	let newMode = $state<'STANDARD' | 'ALLOW_ALL'>('STANDARD');
	let newDeletionProtection = $state(true);
	let creating = $state(false);
	let deleteTarget = $state<LedgerSummary | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	$effect(() => {
		void load();
	});

	async function load() {
		loading = true;
		try {
			rows = await listLedgers();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load ledgers');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!newName.trim()) return toast.error('Ledger name is required.');
		creating = true;
		try {
			await createLedger(newName.trim(), newMode, newDeletionProtection);
			toast.success(`Created ledger "${newName.trim()}".`);
			newName = '';
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create ledger');
		} finally {
			creating = false;
		}
	}

	async function disableProtection(l: LedgerSummary) {
		try {
			await updateLedgerProtection(l.name, false);
			toast.success(`Disabled deletion protection on "${l.name}".`);
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to update');
		}
	}

	async function remove(l: LedgerSummary) {
		try {
			const detail = await describeLedger(l.name);
			if (detail.deletionProtection) {
				deleteTarget = l;
				deleteOpen = true;
				return;
			}
			await deleteLedger(l.name);
			toast.success('Ledger deleted.');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}

	async function confirmRemove() {
		if (!deleteTarget) return;
		deleteBusy = true;
		try {
			await updateLedgerProtection(deleteTarget.name, false);
			await deleteLedger(deleteTarget.name);
			toast.success('Ledger deleted.');
			deleteOpen = false;
			deleteTarget = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		} finally {
			deleteBusy = false;
		}
	}

	function timestamp(t: number): string {
		return new Date(t * 1000).toLocaleString();
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Ledgers
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="space-y-2 rounded-md border border-border p-3">
		<div class="text-xs font-semibold">Create ledger</div>
		<div class="grid grid-cols-3 gap-2">
			<Input bind:value={newName} placeholder="ledger name" class="h-8 text-xs col-span-2" />
			<Select
				type="single"
				value={newMode}
				onValueChange={(v) => (newMode = v as 'STANDARD' | 'ALLOW_ALL')}
			>
				<SelectTrigger size="sm" class="w-full text-xs">
					{newMode}
				</SelectTrigger>
				<SelectContent>
					<SelectItem value="STANDARD" label="STANDARD">STANDARD</SelectItem>
					<SelectItem value="ALLOW_ALL" label="ALLOW_ALL">ALLOW_ALL</SelectItem>
				</SelectContent>
			</Select>
		</div>
		<label class="flex items-center gap-2 text-[11px]">
			<input
				type="checkbox"
				class="rounded border-border"
				bind:checked={newDeletionProtection}
			/>
			Deletion protection on
		</label>
		<Button size="sm" onclick={create} disabled={creating}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create ledger'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'state', label: 'State', width: '100px', cell: stateCell },
			{ key: 'creationDateTime', label: 'Created', width: '180px', cell: createdCell },
			{ key: '__actions', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.name}
	>
		{#snippet empty()}
			<EmptyState
				icon={BookTextIcon}
				title="No ledgers"
				description="Create a QLDB ledger. AWSim only stores metadata — no journal storage."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet stateCell(row: LedgerSummary)}
	<Badge
		variant="outline"
		class={row.state === 'ACTIVE'
			? 'h-5 px-2 text-[10px] text-green-500'
			: 'h-5 px-2 text-[10px] text-amber-500'}
	>
		{row.state}
	</Badge>
{/snippet}

{#snippet createdCell(row: LedgerSummary)}
	<span class="text-xs">{timestamp(row.creationDateTime)}</span>
{/snippet}

{#snippet actionsCell(row: LedgerSummary)}
	<div class="flex items-center gap-1">
		<Button
			variant="ghost"
			size="xs"
			onclick={() => disableProtection(row)}
			title="Disable deletion protection"
		>
			<LockOpenIcon />
		</Button>
		<Button variant="ghost" size="xs" onclick={() => remove(row)}>
			<Trash2Icon class="text-destructive" />
		</Button>
	</div>
{/snippet}

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete ledger?"
	description={`Ledger "${deleteTarget?.name ?? ''}" has DeletionProtection on. Confirm to disable deletion protection and delete it now.`}
	busy={deleteBusy}
	onConfirm={confirmRemove}
	onClose={() => {
		deleteOpen = false;
		deleteTarget = null;
	}}
/>
