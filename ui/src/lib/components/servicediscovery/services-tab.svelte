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
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
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
	let deleteTarget = $state<SDService | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

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

	let selectedNsLabel = $derived(namespaces.find((n) => n.id === selectedNs)?.name ?? '');

	function remove(s: SDService) {
		deleteTarget = s;
		deleteOpen = true;
	}

	async function confirmRemove() {
		if (!deleteTarget) return;
		deleteBusy = true;
		try {
			await deleteService(deleteTarget.id);
			toast.success('Service deleted.');
			deleteOpen = false;
			deleteTarget = null;
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete service');
		} finally {
			deleteBusy = false;
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="flex items-center gap-2">
			<label for="sd-ns" class="text-xs text-muted-foreground">Namespace</label>
			<Select type="single" bind:value={selectedNs}>
				<SelectTrigger id="sd-ns" size="sm" class="h-7 w-[160px] text-xs">
					{selectedNsLabel}
				</SelectTrigger>
				<SelectContent>
					{#each namespaces as n (n.id)}
						<SelectItem value={n.id} label={n.name}>{n.name}</SelectItem>
					{/each}
				</SelectContent>
			</Select>
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

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete service?"
	description={`Delete service "${deleteTarget?.name ?? ''}". Instances must be removed first.`}
	busy={deleteBusy}
	onConfirm={confirmRemove}
	onClose={() => {
		deleteOpen = false;
		deleteTarget = null;
	}}
/>
