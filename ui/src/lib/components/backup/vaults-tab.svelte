<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import { toast } from 'svelte-sonner';
	import { listVaults, createVault, deleteVault, type BackupVault } from '$lib/api/backup';

	interface Props {
		refreshKey?: number;
		onChanged?: () => void;
	}

	let { refreshKey = 0, onChanged }: Props = $props();

	let rows = $state<BackupVault[]>([]);
	let loading = $state(false);
	let newName = $state('');
	let creating = $state(false);

	$effect(() => {
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			rows = await listVaults();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load vaults');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!newName.trim()) return toast.error('Vault name is required.');
		creating = true;
		try {
			await createVault(newName.trim());
			toast.success(`Created vault "${newName.trim()}".`);
			newName = '';
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create vault');
		} finally {
			creating = false;
		}
	}

	async function remove(v: BackupVault) {
		if (!confirm(`Delete vault "${v.name}"? Recovery points must be removed first.`)) return;
		try {
			await deleteVault(v.name);
			toast.success('Vault deleted.');
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete vault');
		}
	}

	function timestamp(t: number): string {
		return new Date(t * 1000).toLocaleString();
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Vaults
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="flex items-center gap-2">
		<Input bind:value={newName} placeholder="vault-name" class="h-8 max-w-[260px]" />
		<Button size="sm" onclick={create} disabled={creating}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create vault'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'numberOfRecoveryPoints', label: 'Recovery points', width: '140px' },
			{ key: 'locked', label: 'Lock', width: '80px', cell: lockCell },
			{ key: 'creationDate', label: 'Created', width: '180px', cell: createdCell },
			{ key: 'name', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.name}
	>
		{#snippet empty()}
			<EmptyState
				icon={ShieldCheckIcon}
				title="No vaults"
				description="Create a backup vault to hold recovery points from EFS, RDS, DynamoDB, S3, or other resources."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet lockCell(row: BackupVault)}
	{#if row.locked}
		<Badge variant="outline" class="h-5 px-2 text-[10px] text-blue-500">locked</Badge>
	{:else}
		<span class="text-xs text-muted-foreground">—</span>
	{/if}
{/snippet}

{#snippet createdCell(row: BackupVault)}
	<span class="text-xs">{timestamp(row.creationDate)}</span>
{/snippet}

{#snippet actionsCell(row: BackupVault)}
	<Button variant="ghost" size="xs" onclick={() => remove(row)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}
