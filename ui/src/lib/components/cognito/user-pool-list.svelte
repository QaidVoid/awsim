<script lang="ts">
	import { listUserPools, deleteUserPool, type UserPool } from '$lib/api/cognito';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuItem,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';
	import Users from '@lucide/svelte/icons/users';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import MoreHorizontal from '@lucide/svelte/icons/more-horizontal';
	import CreatePoolDialog from './create-pool-dialog.svelte';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';

	interface Props {
		onSelect: (pool: UserPool) => void;
	}

	let { onSelect }: Props = $props();

	let pools = $state<UserPool[]>([]);
	let loading = $state(false);
	let filter = $state('');

	let createOpen = $state(false);
	let deletePool = $state<UserPool | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	const filtered = $derived(
		filter.trim()
			? pools.filter(
					(p) =>
						p.name.toLowerCase().includes(filter.trim().toLowerCase()) ||
						p.id.includes(filter.trim())
				)
			: pools
	);

	let nextToken = $state<string | undefined>(undefined);
	let loadingMore = $state(false);

	async function load() {
		loading = true;
		try {
			const page = await listUserPools({ maxResults: 60 });
			pools = page.pools;
			nextToken = page.nextToken;
		} finally {
			loading = false;
		}
	}

	async function loadMore() {
		if (!nextToken || loadingMore) return;
		loadingMore = true;
		try {
			const page = await listUserPools({ maxResults: 60, nextToken });
			pools = [...pools, ...page.pools];
			nextToken = page.nextToken;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Load more failed');
		} finally {
			loadingMore = false;
		}
	}

	async function confirmDelete() {
		if (!deletePool) return;
		deleteBusy = true;
		try {
			await deleteUserPool(deletePool.id);
			toast.success(`Deleted ${deletePool.name}`);
			deleteOpen = false;
			deletePool = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}

	onMount(load);
</script>

{#snippet actionsCell(p: UserPool)}
	<div
		class="flex justify-end"
		role="presentation"
		onclick={(e) => e.stopPropagation()}
		onkeydown={(e) => e.stopPropagation()}
	>
		<DropdownMenu>
			<DropdownMenuTrigger>
				<Button variant="ghost" size="icon-sm" aria-label="Pool actions">
					<MoreHorizontal class="size-4" />
				</Button>
			</DropdownMenuTrigger>
			<DropdownMenuContent align="end">
				<DropdownMenuItem onclick={() => onSelect(p)}>Open</DropdownMenuItem>
				<DropdownMenuItem
					class="text-destructive focus:text-destructive"
					onclick={() => {
						deletePool = p;
						deleteOpen = true;
					}}
				>
					Delete
				</DropdownMenuItem>
			</DropdownMenuContent>
		</DropdownMenu>
	</div>
{/snippet}

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input
			type="search"
			placeholder="Filter user pools..."
			bind:value={filter}
			class="h-8 max-w-xs"
		/>
		<div class="flex-1"></div>
		<Badge variant="secondary">
			{filtered.length} of {pools.length}{nextToken ? '+' : ''}
		</Badge>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
		<Button size="xs" onclick={() => (createOpen = true)}>
			<Plus class="size-3.5" /> Pool
		</Button>
	</div>
	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
			{loading}
			columns={[
				{ key: 'name', label: 'Name', width: '28%' },
				{ key: 'id', label: 'Pool ID', mono: true, width: '32%' },
				{ key: 'status', label: 'Status', width: '14%' },
				{ key: 'creationDate', label: 'Created', width: '18%' },
				{ key: 'actions', label: '', width: '8%', align: 'right', cell: actionsCell }
			]}
			rowKey={(r: UserPool) => r.id}
			onRowClick={onSelect}
		>
			{#snippet empty()}
				<EmptyState
					icon={Users}
					title="No user pools"
					description="A Cognito user pool is a managed user directory that handles sign-up, sign-in, and access tokens for your applications."
				>
					{#snippet action()}
						<Button size="sm" onclick={() => (createOpen = true)}>
							<Plus class="size-3.5" />
							Create pool
						</Button>
					{/snippet}
				</EmptyState>
			{/snippet}
		</DataTable>
	</div>
	{#if nextToken}
		<div class="flex justify-center border-t border-border px-6 py-3">
			<Button variant="outline" size="xs" onclick={loadMore} disabled={loadingMore}>
				{loadingMore ? 'Loading...' : 'Load more pools'}
			</Button>
		</div>
	{/if}
</div>

<CreatePoolDialog
	bind:open={createOpen}
	onClose={() => (createOpen = false)}
	onCreated={() => void load()}
/>
{#if deletePool}
	<ConfirmDialog
		bind:open={deleteOpen}
		title="Delete user pool"
		description={`Permanently delete pool "${deletePool.name}" (${deletePool.id})? All users, groups, and clients are removed.`}
		busy={deleteBusy}
		onConfirm={confirmDelete}
		onClose={() => {
			deleteOpen = false;
			deletePool = null;
		}}
	/>
{/if}
