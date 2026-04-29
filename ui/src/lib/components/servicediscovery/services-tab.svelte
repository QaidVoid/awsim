<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import NetworkIcon from '@lucide/svelte/icons/network';
	import { toast } from 'svelte-sonner';
	import {
		listNamespaces,
		listServices,
		createService,
		deleteService,
		type Namespace,
		type SDService
	} from '$lib/api/servicediscovery';

	interface Props {
		refreshKey?: number;
		onSelect: (svc: SDService) => void;
		onChanged?: () => void;
	}

	let { refreshKey = 0, onSelect, onChanged }: Props = $props();

	let namespaces = $state<Namespace[]>([]);
	let rows = $state<SDService[]>([]);
	let loading = $state(false);
	let selectedNs = $state('');
	let newName = $state('');
	let creating = $state(false);

	$effect(() => {
		selectedNs;
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			namespaces = await listNamespaces();
			if (!selectedNs && namespaces[0]) selectedNs = namespaces[0].id;
			rows = selectedNs ? await listServices(selectedNs) : [];
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load services');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!selectedNs) return toast.error('Select a namespace first.');
		if (!newName.trim()) return toast.error('Service name is required.');
		creating = true;
		try {
			await createService(selectedNs, newName.trim());
			toast.success(`Created service "${newName.trim()}".`);
			newName = '';
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create service');
		} finally {
			creating = false;
		}
	}

	async function remove(s: SDService) {
		if (!confirm(`Delete service "${s.name}"? Instances must be removed first.`)) return;
		try {
			await deleteService(s.id);
			toast.success('Service deleted.');
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete service');
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="flex items-center gap-2">
			<label for="sd-ns" class="text-xs text-muted-foreground">Namespace</label>
			<select
				id="sd-ns"
				bind:value={selectedNs}
				class="h-7 rounded-md border border-border bg-background px-2 text-xs"
			>
				{#each namespaces as n (n.id)}
					<option value={n.id}>{n.name}</option>
				{/each}
			</select>
		</div>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="flex items-center gap-2">
		<Input bind:value={newName} placeholder="service name" class="h-8 max-w-[220px]" />
		<Button size="sm" onclick={create} disabled={creating || !selectedNs}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create service'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		onRowClick={onSelect}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'id', label: 'ID', mono: true, width: '200px' },
			{ key: 'type', label: 'Type', width: '80px', cell: typeCell },
			{ key: 'instanceCount', label: 'Instances', width: '100px' },
			{ key: '__actions', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.id}
	>
		{#snippet empty()}
			<EmptyState
				icon={NetworkIcon}
				title="No services"
				description={selectedNs
					? 'Create a service in this namespace, then register instances under it.'
					: 'Create a namespace first.'}
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet typeCell(row: SDService)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">{row.type}</Badge>
{/snippet}

{#snippet actionsCell(row: SDService)}
	<Button variant="ghost" size="xs" onclick={() => remove(row)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}
