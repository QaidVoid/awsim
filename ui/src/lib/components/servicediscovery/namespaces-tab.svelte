<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import MapPinIcon from '@lucide/svelte/icons/map-pin';
	import { toast } from 'svelte-sonner';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import {
		listNamespaces,
		createHttpNamespace,
		createPrivateDnsNamespace,
		createPublicDnsNamespace,
		deleteNamespace,
		type Namespace
	} from '$lib/api/servicediscovery';

	interface Props {
		refreshKey?: number;
		onChanged?: () => void;
	}

	let { refreshKey = 0, onChanged }: Props = $props();

	let rows = $state<Namespace[]>([]);
	let loading = $state(false);
	let newName = $state('');
	let newType = $state<'HTTP' | 'DNS_PRIVATE' | 'DNS_PUBLIC'>('HTTP');
	let creating = $state(false);
	let deleteTarget = $state<Namespace | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	$effect(() => {
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			rows = await listNamespaces();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load namespaces');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!newName.trim()) return toast.error('Namespace name is required.');
		creating = true;
		try {
			if (newType === 'HTTP') await createHttpNamespace(newName.trim());
			else if (newType === 'DNS_PRIVATE') await createPrivateDnsNamespace(newName.trim());
			else await createPublicDnsNamespace(newName.trim());
			toast.success(`Created ${newType} namespace "${newName.trim()}".`);
			newName = '';
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create namespace');
		} finally {
			creating = false;
		}
	}

	function remove(n: Namespace) {
		deleteTarget = n;
		deleteOpen = true;
	}

	async function confirmRemove() {
		if (!deleteTarget) return;
		deleteBusy = true;
		try {
			await deleteNamespace(deleteTarget.id);
			toast.success('Namespace deleted.');
			deleteOpen = false;
			deleteTarget = null;
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete namespace');
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
			Namespaces
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="flex flex-wrap items-center gap-2">
		<Input bind:value={newName} placeholder="namespace name" class="h-8 max-w-[220px]" />
		<Select
			type="single"
			value={newType}
			onValueChange={(v) => (newType = v as 'HTTP' | 'DNS_PRIVATE' | 'DNS_PUBLIC')}
		>
			<SelectTrigger size="sm" class="w-[160px] text-xs">
				{newType}
			</SelectTrigger>
			<SelectContent>
				<SelectItem value="HTTP" label="HTTP">HTTP</SelectItem>
				<SelectItem value="DNS_PRIVATE" label="DNS_PRIVATE">DNS_PRIVATE</SelectItem>
				<SelectItem value="DNS_PUBLIC" label="DNS_PUBLIC">DNS_PUBLIC</SelectItem>
			</SelectContent>
		</Select>
		<Button size="sm" onclick={create} disabled={creating}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create namespace'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'id', label: 'ID', mono: true, width: '200px' },
			{ key: 'type', label: 'Type', width: '110px', cell: typeCell },
			{ key: 'serviceCount', label: 'Services', width: '90px' },
			{ key: 'createDate', label: 'Created', width: '180px', cell: createdCell },
			{ key: '__actions', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.id}
	>
		{#snippet empty()}
			<EmptyState
				icon={MapPinIcon}
				title="No namespaces"
				description="Create an HTTP, private-DNS, or public-DNS namespace before registering services."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet typeCell(row: Namespace)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">{row.type}</Badge>
{/snippet}

{#snippet createdCell(row: Namespace)}
	<span class="text-xs">{timestamp(row.createDate)}</span>
{/snippet}

{#snippet actionsCell(row: Namespace)}
	<Button variant="ghost" size="xs" onclick={() => remove(row)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete namespace?"
	description={`Delete namespace "${deleteTarget?.name ?? ''}". Services must be removed first.`}
	busy={deleteBusy}
	onConfirm={confirmRemove}
	onClose={() => {
		deleteOpen = false;
		deleteTarget = null;
	}}
/>
