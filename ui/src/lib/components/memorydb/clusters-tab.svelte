<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import MemoryStickIcon from '@lucide/svelte/icons/memory-stick';
	import { toast } from 'svelte-sonner';
	import {
		describeClusters,
		createCluster,
		deleteCluster,
		describeAcls,
		type Cluster,
		type Acl
	} from '$lib/api/memorydb';

	interface Props {
		refreshKey?: number;
		onChanged?: () => void;
	}

	let { refreshKey = 0, onChanged }: Props = $props();

	let rows = $state<Cluster[]>([]);
	let acls = $state<Acl[]>([]);
	let loading = $state(false);
	let newName = $state('');
	let newNode = $state('db.t4g.small');
	let newAcl = $state('');
	let newShards = $state('1');
	let creating = $state(false);

	$effect(() => {
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			[rows, acls] = await Promise.all([describeClusters(), describeAcls()]);
			if (!newAcl && acls[0]) newAcl = acls[0].name;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load clusters');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!newName.trim()) return toast.error('Cluster name is required.');
		if (!newAcl) return toast.error('ACL is required (create an ACL first).');
		creating = true;
		try {
			await createCluster({
				clusterName: newName.trim(),
				nodeType: newNode.trim(),
				aclName: newAcl,
				numShards: parseInt(newShards, 10) || 1
			});
			toast.success(`Created cluster "${newName.trim()}".`);
			newName = '';
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create cluster');
		} finally {
			creating = false;
		}
	}

	async function remove(c: Cluster) {
		if (!confirm(`Delete cluster "${c.name}"?`)) return;
		try {
			await deleteCluster(c.name);
			toast.success('Cluster deleted.');
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete cluster');
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Clusters
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="space-y-2 rounded-md border border-border p-3">
		<div class="text-xs font-semibold">Create cluster</div>
		<div class="grid grid-cols-2 gap-2">
			<Input bind:value={newName} placeholder="cluster name" class="h-8 text-xs" />
			<Input bind:value={newNode} placeholder="db.t4g.small" class="h-8 font-mono text-xs" />
			<select
				bind:value={newAcl}
				class="h-8 rounded-md border border-border bg-background px-2 text-xs"
			>
				<option value="">Pick ACL…</option>
				{#each acls as a (a.name)}
					<option value={a.name}>{a.name}</option>
				{/each}
			</select>
			<Input bind:value={newShards} placeholder="shards (1)" class="h-8 text-xs" type="number" min="1" />
		</div>
		<Button size="sm" onclick={create} disabled={creating}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create cluster'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'nodeType', label: 'Node type', width: '160px', mono: true },
			{ key: 'engineVersion', label: 'Version', width: '80px' },
			{ key: 'numberOfShards', label: 'Shards', width: '70px' },
			{ key: 'aclName', label: 'ACL', width: '160px', mono: true },
			{ key: 'status', label: 'Status', width: '110px', cell: stateCell },
			{ key: 'name', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.name}
	>
		{#snippet empty()}
			<EmptyState
				icon={MemoryStickIcon}
				title="No clusters"
				description="Create a MemoryDB cluster to back Redis-compatible workloads."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet stateCell(row: Cluster)}
	<Badge
		variant="outline"
		class={row.status === 'available'
			? 'h-5 px-2 text-[10px] text-green-500'
			: 'h-5 px-2 text-[10px] text-amber-500'}
	>
		{row.status}
	</Badge>
{/snippet}

{#snippet actionsCell(row: Cluster)}
	<Button variant="ghost" size="xs" onclick={() => remove(row)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}
