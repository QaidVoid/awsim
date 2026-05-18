<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import UserIcon from '@lucide/svelte/icons/user';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import { toast } from 'svelte-sonner';
	import { describeUsers, createUser, deleteUser, type User } from '$lib/api/memorydb';

	interface Props {
		refreshKey?: number;
		onChanged?: () => void;
	}

	let { refreshKey = 0, onChanged }: Props = $props();

	let rows = $state<User[]>([]);
	let loading = $state(false);
	let newName = $state('');
	let newAccess = $state('on ~* +@all');
	let creating = $state(false);
	let deleteTarget = $state<User | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	$effect(() => {
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			rows = await describeUsers();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load users');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!newName.trim()) return toast.error('Username is required.');
		creating = true;
		try {
			await createUser(newName.trim(), newAccess.trim() || 'on ~* +@all');
			toast.success(`Created user "${newName.trim()}".`);
			newName = '';
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create user');
		} finally {
			creating = false;
		}
	}

	function remove(u: User) {
		deleteTarget = u;
		deleteOpen = true;
	}

	async function confirmRemove() {
		const u = deleteTarget;
		if (!u) return;
		deleteBusy = true;
		try {
			await deleteUser(u.name);
			toast.success('User deleted.');
			deleteOpen = false;
			deleteTarget = null;
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete user');
		} finally {
			deleteBusy = false;
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Users
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="space-y-2 rounded-md border border-border p-3">
		<div class="text-xs font-semibold">Create user</div>
		<div class="grid grid-cols-2 gap-2">
			<Input bind:value={newName} placeholder="username" class="h-8 text-xs" />
			<Input
				bind:value={newAccess}
				placeholder="access string (on ~* +@all)"
				class="h-8 font-mono text-xs"
			/>
		</div>
		<Button size="sm" onclick={create} disabled={creating}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create user'}
		</Button>
		<p class="text-[11px] text-muted-foreground">
			A dummy password is generated client-side; AWSim does not enforce auth.
		</p>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'accessString', label: 'Access', mono: true },
			{ key: 'status', label: 'Status', width: '110px', cell: statusCell },
			{ key: '__actions', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.name}
	>
		{#snippet empty()}
			<EmptyState icon={UserIcon} title="No users" description="Create a user to attach to ACLs." />
		{/snippet}
	</DataTable>
</div>

{#snippet statusCell(row: User)}
	<Badge variant="outline" class="h-5 px-2 text-[10px]">{row.status}</Badge>
{/snippet}

{#snippet actionsCell(row: User)}
	<Button variant="ghost" size="xs" onclick={() => remove(row)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete user?"
	description={`Delete user "${deleteTarget?.name ?? ''}".`}
	busy={deleteBusy}
	onConfirm={confirmRemove}
	onClose={() => (deleteOpen = false)}
/>
