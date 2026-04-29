<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import SnowflakeIcon from '@lucide/svelte/icons/snowflake';
	import { toast } from 'svelte-sonner';
	import { listVaults, createVault, deleteVault, type Vault } from '$lib/api/glacier';

	interface Props {
		onSelect: (v: Vault) => void;
		refreshKey?: number;
		onChanged?: () => void;
	}

	let { onSelect, refreshKey = 0, onChanged }: Props = $props();

	let rows = $state<Vault[]>([]);
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

	async function remove(v: Vault, ev: MouseEvent) {
		ev.stopPropagation();
		if (!confirm(`Delete vault "${v.vaultName}"? Archives must be removed first.`)) return;
		try {
			await deleteVault(v.vaultName);
			toast.success('Vault deleted.');
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete vault');
		}
	}

	function fmtBytes(n: number): string {
		if (n < 1024) return `${n} B`;
		if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
		if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
		return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
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
		<Input bind:value={newName} placeholder="vault name" class="h-8 max-w-[260px]" />
		<Button size="sm" onclick={create} disabled={creating}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create vault'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		onRowClick={onSelect}
		columns={[
			{ key: 'vaultName', label: 'Name', mono: true },
			{ key: 'numberOfArchives', label: 'Archives', width: '100px' },
			{ key: 'sizeInBytes', label: 'Size', width: '110px', cell: sizeCell },
			{ key: 'creationDate', label: 'Created', width: '220px' },
			{ key: '__actions', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.vaultName}
	>
		{#snippet empty()}
			<EmptyState
				icon={SnowflakeIcon}
				title="No vaults"
				description="Create a Glacier vault to upload archives. Note: vaults can only be deleted when empty."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet sizeCell(row: Vault)}
	<span class="font-mono text-xs">{fmtBytes(row.sizeInBytes)}</span>
{/snippet}

{#snippet actionsCell(row: Vault)}
	<Button variant="ghost" size="xs" onclick={(e) => remove(row, e)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}
