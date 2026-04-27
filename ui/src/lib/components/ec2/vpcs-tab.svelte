<script lang="ts">
	import { createVpc, deleteVpc, type Vpc } from '$lib/api/ec2';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter
	} from '$lib/components/ui/dialog';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { toast } from 'svelte-sonner';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import NetworkIcon from '@lucide/svelte/icons/network';

	interface Props {
		vpcs: Vpc[];
		loading: boolean;
		onReload: () => void;
	}

	let { vpcs, loading, onReload }: Props = $props();

	let createOpen = $state(false);
	let formCidr = $state('10.0.0.0/16');
	let creating = $state(false);

	async function handleCreate(e: Event) {
		e.preventDefault();
		if (!formCidr.trim()) return;
		creating = true;
		try {
			await createVpc(formCidr.trim());
			toast.success(`Created VPC ${formCidr.trim()}`);
			formCidr = '10.0.0.0/16';
			createOpen = false;
			onReload();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Create failed');
		} finally {
			creating = false;
		}
	}

	async function handleDelete(vpc: Vpc) {
		if (!confirm(`Delete VPC ${vpc.vpcId}?`)) return;
		try {
			await deleteVpc(vpc.vpcId);
			toast.success(`Deleted ${vpc.vpcId}`);
			onReload();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		}
	}

	function stateVariant(state: string): 'default' | 'secondary' | 'outline' {
		if (state === 'available') return 'default';
		return 'outline';
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header
		class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2"
	>
		<div class="text-xs text-muted-foreground">
			{vpcs.length} VPC{vpcs.length === 1 ? '' : 's'}
		</div>
		<div class="flex items-center gap-2">
			<Button variant="outline" size="sm" onclick={onReload} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={() => (createOpen = true)}>
				<PlusIcon />
				Create VPC
			</Button>
		</div>
	</header>

	<div class="min-h-0 flex-1">
		<DataTable
			rows={vpcs}
			{loading}
			rowKey={(r) => r.vpcId}
			columns={[
				{ key: 'vpcId', label: 'VPC ID', mono: true },
				{ key: 'cidrBlock', label: 'CIDR', mono: true },
				{ key: 'state', label: 'State', cell: stateCell },
				{ key: 'isDefault', label: 'Default', cell: defaultCell },
				{ key: 'instanceTenancy', label: 'Tenancy' },
				{ key: 'actions', label: '', align: 'right', width: '60px', cell: actionsCell }
			]}
		>
			{#snippet empty()}
				<EmptyState
					icon={NetworkIcon}
					title="No VPCs"
					description="Create a Virtual Private Cloud to host your network resources."
				>
					{#snippet action()}
						<Button onclick={() => (createOpen = true)}>
							<PlusIcon />
							Create VPC
						</Button>
					{/snippet}
				</EmptyState>
			{/snippet}
		</DataTable>
	</div>
</div>

{#snippet stateCell(row: Vpc)}
	<Badge variant={stateVariant(row.state)}>{row.state}</Badge>
{/snippet}
{#snippet defaultCell(row: Vpc)}
	<span class="text-xs text-muted-foreground">{row.isDefault ? 'Yes' : 'No'}</span>
{/snippet}
{#snippet actionsCell(row: Vpc)}
	<Button
		type="button"
		variant="ghost"
		size="icon-xs"
		onclick={() => handleDelete(row)}
		disabled={row.isDefault}
		aria-label="Delete VPC"
	>
		<Trash2Icon />
	</Button>
{/snippet}

<Dialog open={createOpen} onOpenChange={(o) => (createOpen = o)}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Create VPC</DialogTitle>
			<DialogDescription>Provision a new Virtual Private Cloud.</DialogDescription>
		</DialogHeader>
		<form onsubmit={handleCreate} class="flex flex-col gap-4 py-2">
			<div class="flex flex-col gap-1.5">
				<Label for="vpc-cidr">CIDR block</Label>
				<Input id="vpc-cidr" bind:value={formCidr} placeholder="10.0.0.0/16" required />
			</div>
			<DialogFooter>
				<Button type="button" variant="ghost" onclick={() => (createOpen = false)}>Cancel</Button>
				<Button type="submit" disabled={creating || !formCidr.trim()}>
					<PlusIcon />
					{creating ? 'Creating...' : 'Create'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
