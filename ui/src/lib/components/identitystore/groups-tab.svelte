<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import UsersRoundIcon from '@lucide/svelte/icons/users-round';
	import { toast } from 'svelte-sonner';
	import {
		listGroups,
		createGroup,
		deleteGroup,
		listGroupMemberships,
		createGroupMembership,
		deleteGroupMembership,
		listUsers,
		type IdGroup,
		type IdUser,
		type GroupMembership
	} from '$lib/api/identitystore';

	interface Props {
		identityStoreId: string;
		refreshKey?: number;
		onChanged?: () => void;
	}

	let { identityStoreId, refreshKey = 0, onChanged }: Props = $props();

	let rows = $state<IdGroup[]>([]);
	let users = $state<IdUser[]>([]);
	let memberships = $state<Record<string, GroupMembership[]>>({});
	let loading = $state(false);
	let newDisplay = $state('');
	let creating = $state(false);
	let memberPicks = $state<Record<string, string>>({});

	$effect(() => {
		identityStoreId;
		refreshKey;
		void load();
	});

	async function load() {
		if (!identityStoreId) return;
		loading = true;
		try {
			[rows, users] = await Promise.all([
				listGroups(identityStoreId),
				listUsers(identityStoreId)
			]);
			const map: Record<string, GroupMembership[]> = {};
			await Promise.all(
				rows.map(async (g) => {
					map[g.groupId] = await listGroupMemberships(identityStoreId, g.groupId);
				})
			);
			memberships = map;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load groups');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!newDisplay.trim()) return toast.error('DisplayName is required.');
		creating = true;
		try {
			await createGroup({ identityStoreId, displayName: newDisplay.trim() });
			toast.success(`Created group "${newDisplay.trim()}".`);
			newDisplay = '';
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create group');
		} finally {
			creating = false;
		}
	}

	async function remove(g: IdGroup) {
		if (!confirm(`Delete group "${g.displayName}"? Memberships are cascaded.`)) return;
		try {
			await deleteGroup(identityStoreId, g.groupId);
			toast.success('Group deleted.');
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}

	async function addMember(g: IdGroup) {
		const userId = memberPicks[g.groupId];
		if (!userId) return toast.error('Pick a user.');
		try {
			await createGroupMembership({
				identityStoreId,
				groupId: g.groupId,
				userId
			});
			toast.success('Member added.');
			memberPicks[g.groupId] = '';
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to add member');
		}
	}

	async function removeMember(m: GroupMembership) {
		if (!confirm('Remove member?')) return;
		try {
			await deleteGroupMembership(identityStoreId, m.membershipId);
			toast.success('Member removed.');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to remove member');
		}
	}

	function userName(id: string): string {
		return users.find((u) => u.userId === id)?.userName ?? id.slice(0, 12);
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Groups
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="flex items-center gap-2">
		<Input bind:value={newDisplay} placeholder="display name" class="h-8 max-w-[260px] text-xs" />
		<Button size="sm" onclick={create} disabled={creating}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create group'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'displayName', label: 'Display name', mono: true },
			{ key: 'groupId', label: 'GroupId', mono: true, width: '300px' },
			{ key: 'groupId', label: 'Members', cell: membersCell },
			{ key: 'groupId', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.groupId}
	>
		{#snippet empty()}
			<EmptyState
				icon={UsersRoundIcon}
				title="No groups"
				description="Create a group to assign permission sets via SSO Admin."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet membersCell(row: IdGroup)}
	<div class="flex flex-col gap-1">
		<div class="flex flex-wrap gap-1">
			{#each memberships[row.groupId] ?? [] as m (m.membershipId)}
				<button
					type="button"
					onclick={() => removeMember(m)}
					title="Remove"
					class="group flex items-center gap-1 rounded border border-border bg-muted/40 px-1.5 py-0.5 font-mono text-[10px] hover:border-destructive/50 hover:text-destructive"
				>
					{userName(m.memberUserId)}
					<Trash2Icon class="size-2.5 opacity-0 group-hover:opacity-100" />
				</button>
			{/each}
		</div>
		{#if users.length > 0}
			<div class="flex items-center gap-1">
				<select
					bind:value={memberPicks[row.groupId]}
					class="h-6 rounded border border-border bg-background px-1 text-[10px]"
				>
					<option value="">add user…</option>
					{#each users as u (u.userId)}
						<option value={u.userId}>{u.userName}</option>
					{/each}
				</select>
				<Button variant="outline" size="xs" onclick={() => addMember(row)}>
					<PlusIcon class="size-3" />
				</Button>
			</div>
		{/if}
	</div>
{/snippet}

{#snippet actionsCell(row: IdGroup)}
	<Button variant="ghost" size="xs" onclick={() => remove(row)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}
