<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import UsersIcon from '@lucide/svelte/icons/users';
	import { toast } from 'svelte-sonner';
	import { listUsers, createUser, deleteUser, type IdUser } from '$lib/api/identitystore';

	interface Props {
		identityStoreId: string;
		refreshKey?: number;
		onChanged?: () => void;
	}

	let { identityStoreId, refreshKey = 0, onChanged }: Props = $props();

	let rows = $state<IdUser[]>([]);
	let loading = $state(false);
	let newName = $state('');
	let newDisplay = $state('');
	let newEmail = $state('');
	let creating = $state(false);

	$effect(() => {
		identityStoreId;
		refreshKey;
		void load();
	});

	async function load() {
		if (!identityStoreId) return;
		loading = true;
		try {
			rows = await listUsers(identityStoreId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load users');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!newName.trim()) return toast.error('UserName is required.');
		creating = true;
		try {
			await createUser({
				identityStoreId,
				userName: newName.trim(),
				displayName: newDisplay.trim() || undefined,
				email: newEmail.trim() || undefined
			});
			toast.success(`Created user "${newName.trim()}".`);
			newName = '';
			newDisplay = '';
			newEmail = '';
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create user');
		} finally {
			creating = false;
		}
	}

	async function remove(u: IdUser) {
		if (!confirm(`Delete user "${u.userName}"? Memberships are cascaded.`)) return;
		try {
			await deleteUser(identityStoreId, u.userId);
			toast.success('User deleted.');
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
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
		<div class="grid grid-cols-3 gap-2">
			<Input bind:value={newName} placeholder="username" class="h-8 text-xs" />
			<Input bind:value={newDisplay} placeholder="display name" class="h-8 text-xs" />
			<Input bind:value={newEmail} placeholder="email" class="h-8 font-mono text-xs" />
		</div>
		<Button size="sm" onclick={create} disabled={creating}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create user'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'userName', label: 'UserName', mono: true },
			{ key: 'displayName', label: 'Display name' },
			{ key: 'emails', label: 'Email', cell: emailCell },
			{ key: 'userId', label: 'UserId', mono: true, width: '300px' },
			{ key: '__actions', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.userId}
	>
		{#snippet empty()}
			<EmptyState
				icon={UsersIcon}
				title="No users"
				description="Create a user in this Identity Store to participate in groups + permission sets."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet emailCell(row: IdUser)}
	{#if row.emails.length > 0}
		<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">{row.emails[0]}</Badge>
	{:else}
		<span class="text-xs text-muted-foreground">—</span>
	{/if}
{/snippet}

{#snippet actionsCell(row: IdUser)}
	<Button variant="ghost" size="xs" onclick={() => remove(row)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}
