<script lang="ts">
	import {
		createSecurityGroup,
		deleteSecurityGroup,
		type SecurityGroup,
		type Vpc
	} from '$lib/api/ec2';
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
	import ShieldIcon from '@lucide/svelte/icons/shield';

	interface Props {
		groups: SecurityGroup[];
		vpcs: Vpc[];
		loading: boolean;
		onReload: () => void;
	}

	let { groups, vpcs, loading, onReload }: Props = $props();

	let createOpen = $state(false);
	let formName = $state('');
	let formDesc = $state('');
	let formVpcId = $state('');
	let creating = $state(false);

	async function handleCreate(e: Event) {
		e.preventDefault();
		if (!formName.trim() || !formDesc.trim() || !formVpcId) return;
		creating = true;
		try {
			await createSecurityGroup(formName.trim(), formDesc.trim(), formVpcId);
			toast.success(`Created security group ${formName.trim()}`);
			formName = '';
			formDesc = '';
			formVpcId = '';
			createOpen = false;
			onReload();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Create failed');
		} finally {
			creating = false;
		}
	}

	async function handleDelete(group: SecurityGroup) {
		if (!confirm(`Delete security group ${group.groupName}?`)) return;
		try {
			await deleteSecurityGroup(group.groupId);
			toast.success(`Deleted ${group.groupName}`);
			onReload();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header
		class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2"
	>
		<div class="text-xs text-muted-foreground">
			{groups.length} group{groups.length === 1 ? '' : 's'}
		</div>
		<div class="flex items-center gap-2">
			<Button variant="outline" size="sm" onclick={onReload} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={() => (createOpen = true)} disabled={vpcs.length === 0}>
				<PlusIcon />
				Create group
			</Button>
		</div>
	</header>

	<div class="min-h-0 flex-1">
		<DataTable
			rows={groups}
			{loading}
			rowKey={(r) => r.groupId}
			columns={[
				{ key: 'groupName', label: 'Name' },
				{ key: 'groupId', label: 'Group ID', mono: true },
				{ key: 'description', label: 'Description' },
				{ key: 'vpcId', label: 'VPC', mono: true, cell: vpcCell },
				{ key: 'rules', label: 'Rules', cell: rulesCell },
				{ key: 'actions', label: '', align: 'right', width: '60px', cell: actionsCell }
			]}
		>
			{#snippet empty()}
				<EmptyState
					icon={ShieldIcon}
					title="No security groups"
					description="Create a security group to manage inbound and outbound traffic."
				>
					{#snippet action()}
						<Button onclick={() => (createOpen = true)} disabled={vpcs.length === 0}>
							<PlusIcon />
							Create group
						</Button>
					{/snippet}
				</EmptyState>
			{/snippet}
		</DataTable>
	</div>
</div>

{#snippet vpcCell(row: SecurityGroup)}
	<span class="font-mono text-xs">{row.vpcId || '—'}</span>
{/snippet}
{#snippet rulesCell(row: SecurityGroup)}
	<div class="flex items-center gap-1">
		<Badge variant="outline" class="text-[10px]">{row.ingress.length} in</Badge>
		<Badge variant="outline" class="text-[10px]">{row.egress.length} out</Badge>
	</div>
{/snippet}
{#snippet actionsCell(row: SecurityGroup)}
	<Button
		type="button"
		variant="ghost"
		size="icon-xs"
		onclick={() => handleDelete(row)}
		aria-label="Delete security group"
	>
		<Trash2Icon />
	</Button>
{/snippet}

<Dialog open={createOpen} onOpenChange={(o) => (createOpen = o)}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Create security group</DialogTitle>
			<DialogDescription>Group ingress and egress rules attached to a VPC.</DialogDescription>
		</DialogHeader>
		<form onsubmit={handleCreate} class="flex flex-col gap-4 py-2">
			<div class="flex flex-col gap-1.5">
				<Label for="sg-name">Name</Label>
				<Input id="sg-name" bind:value={formName} placeholder="my-sg" required />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="sg-desc">Description</Label>
				<Input id="sg-desc" bind:value={formDesc} placeholder="Allow web traffic" required />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="sg-vpc">VPC</Label>
				<select
					id="sg-vpc"
					bind:value={formVpcId}
					required
					class="h-9 rounded-md border border-input bg-background px-3 py-1 text-sm shadow-xs transition-colors focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring"
				>
					<option value="" disabled>Select a VPC</option>
					{#each vpcs as vpc (vpc.vpcId)}
						<option value={vpc.vpcId}>{vpc.vpcId} ({vpc.cidrBlock})</option>
					{/each}
				</select>
			</div>
			<DialogFooter>
				<Button type="button" variant="ghost" onclick={() => (createOpen = false)}>Cancel</Button>
				<Button type="submit" disabled={creating || !formName.trim() || !formDesc.trim() || !formVpcId}>
					<PlusIcon />
					{creating ? 'Creating...' : 'Create'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
