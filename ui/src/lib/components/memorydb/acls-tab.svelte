<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import { toast } from 'svelte-sonner';
	import { describeAcls, describeUsers, createAcl, deleteAcl, type Acl, type User } from '$lib/api/memorydb';

	interface Props {
		refreshKey?: number;
		onChanged?: () => void;
	}

	let { refreshKey = 0, onChanged }: Props = $props();

	let rows = $state<Acl[]>([]);
	let users = $state<User[]>([]);
	let loading = $state(false);
	let newName = $state('');
	let pickedUsers = $state<Record<string, boolean>>({});
	let creating = $state(false);

	$effect(() => {
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			[rows, users] = await Promise.all([describeAcls(), describeUsers()]);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load ACLs');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!newName.trim()) return toast.error('ACL name is required.');
		const userNames = Object.entries(pickedUsers)
			.filter(([, v]) => v)
			.map(([k]) => k);
		creating = true;
		try {
			await createAcl(newName.trim(), userNames);
			toast.success(`Created ACL "${newName.trim()}".`);
			newName = '';
			pickedUsers = {};
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create ACL');
		} finally {
			creating = false;
		}
	}

	async function remove(a: Acl) {
		if (!confirm(`Delete ACL "${a.name}"?`)) return;
		try {
			await deleteAcl(a.name);
			toast.success('ACL deleted.');
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete ACL');
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			ACLs
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="space-y-2 rounded-md border border-border p-3">
		<div class="text-xs font-semibold">Create ACL</div>
		<Input bind:value={newName} placeholder="acl name" class="h-8 text-xs" />
		{#if users.length > 0}
			<div class="flex flex-wrap gap-2">
				{#each users as u (u.name)}
					<label class="flex items-center gap-1 text-[11px]">
						<input
							type="checkbox"
							class="rounded border-border"
							bind:checked={pickedUsers[u.name]}
						/>
						<span class="font-mono">{u.name}</span>
					</label>
				{/each}
			</div>
		{/if}
		<Button size="sm" onclick={create} disabled={creating}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create ACL'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'userNames', label: 'Users', cell: usersCell },
			{ key: 'status', label: 'Status', width: '110px', cell: statusCell },
			{ key: '__actions', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.name}
	>
		{#snippet empty()}
			<EmptyState icon={KeyRoundIcon} title="No ACLs" description="Create an ACL to gate cluster access." />
		{/snippet}
	</DataTable>
</div>

{#snippet usersCell(row: Acl)}
	<div class="flex flex-wrap gap-1">
		{#each row.userNames as u (u)}
			<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">{u}</Badge>
		{/each}
	</div>
{/snippet}

{#snippet statusCell(row: Acl)}
	<Badge variant="outline" class="h-5 px-2 text-[10px]">{row.status}</Badge>
{/snippet}

{#snippet actionsCell(row: Acl)}
	<Button variant="ghost" size="xs" onclick={() => remove(row)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}
