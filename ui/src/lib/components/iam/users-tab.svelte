<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listUsers,
		getUser,
		deleteUser,
		listAttachedUserPolicies,
		attachUserPolicy,
		detachUserPolicy,
		listUserPolicies,
		getUserPolicy,
		putUserPolicy,
		deleteUserPolicy,
		type IamAttachedPolicy,
		type IamUser,
	} from '$lib/api/iam';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import EntityDetailSheet from './entity-detail-sheet.svelte';
	import CreateEntityDialog from './create-entity-dialog.svelte';
	import EntityPoliciesEditor from './entity-policies-editor.svelte';
	import Users from '@lucide/svelte/icons/users';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { toast } from 'svelte-sonner';

	let users = $state<IamUser[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let selected = $state<IamUser | null>(null);
	let detailLoading = $state(false);
	let createOpen = $state(false);
	let deleting = $state(false);

	let attached = $state<IamAttachedPolicy[]>([]);
	let inlineNames = $state<string[]>([]);

	const filtered = $derived(
		filter.trim()
			? users.filter((u) => u.userName.toLowerCase().includes(filter.trim().toLowerCase()))
			: users
	);

	async function load() {
		loading = true;
		try {
			users = await listUsers();
		} finally {
			loading = false;
		}
	}

	async function openDetail(u: IamUser) {
		selected = u;
		detailLoading = true;
		attached = [];
		inlineNames = [];
		try {
			selected = await getUser(u.userName);
			await reloadPolicies(u.userName);
		} finally {
			detailLoading = false;
		}
	}

	async function reloadPolicies(userName: string) {
		const [a, i] = await Promise.all([
			listAttachedUserPolicies(userName).catch(() => []),
			listUserPolicies(userName).catch(() => []),
		]);
		attached = a;
		inlineNames = i;
	}

	async function handleDelete(u: IamUser) {
		if (!confirm(`Delete user "${u.userName}"? This cannot be undone.`)) return;
		deleting = true;
		try {
			await deleteUser(u.userName);
			toast.success(`Deleted ${u.userName}`);
			selected = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleting = false;
		}
	}

	onMount(load);

	function formatDate(s: string): string {
		try {
			return new Date(s).toLocaleString();
		} catch {
			return s;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input type="search" placeholder="Filter users..." bind:value={filter} class="h-8 max-w-xs" />
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {users.length}</Badge>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<Plus class="size-3.5" />
			<span class="ml-1">New user</span>
		</Button>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
	</div>

	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
			{loading}
			columns={[
				{ key: 'userName', label: 'User name', width: '30%' },
				{ key: 'arn', label: 'ARN', mono: true },
				{ key: 'createDate', label: 'Created', width: '20%' }
			]}
			rowKey={(r: IamUser) => r.arn || r.userName}
			onRowClick={openDetail}
		>
			{#snippet empty()}
				<EmptyState
					icon={Users}
					title="No IAM users"
					description="Create a user with the AWS CLI: aws iam create-user --user-name my-user"
				/>
			{/snippet}
		</DataTable>
	</div>
</div>

<EntityDetailSheet
	open={!!selected}
	onOpenChange={(v) => {
		if (!v) selected = null;
	}}
	title={selected?.userName ?? ''}
	subtitle={selected?.arn}
>
	{#if selected}
		<dl class="grid grid-cols-3 gap-x-4 gap-y-2 py-4 text-sm">
			<dt class="text-muted-foreground">User ID</dt>
			<dd class="col-span-2 font-mono text-xs">{selected.userId}</dd>
			<dt class="text-muted-foreground">Created</dt>
			<dd class="col-span-2">{formatDate(selected.createDate)}</dd>
			<dt class="text-muted-foreground">ARN</dt>
			<dd class="col-span-2 break-all font-mono text-xs">{selected.arn}</dd>
		</dl>
		{#if detailLoading}
			<p class="text-xs text-muted-foreground">Loading details...</p>
		{/if}
		<div class="pt-4">
			<EntityPoliciesEditor
				{attached}
				{inlineNames}
				onAttach={(arn) => attachUserPolicy(selected!.userName, arn)}
				onDetach={(arn) => detachUserPolicy(selected!.userName, arn)}
				onLoadInline={(name) => getUserPolicy(selected!.userName, name)}
				onPutInline={(name, doc) => putUserPolicy(selected!.userName, name, doc)}
				onDeleteInline={(name) => deleteUserPolicy(selected!.userName, name)}
				onMutated={() => selected && reloadPolicies(selected.userName)}
			/>
		</div>
		<div class="flex justify-end pt-4">
			<Button
				variant="destructive"
				size="sm"
				disabled={deleting}
				onclick={() => selected && handleDelete(selected)}
			>
				<Trash2 class="size-4" />
				<span class="ml-1">Delete user</span>
			</Button>
		</div>
	{/if}
</EntityDetailSheet>

<CreateEntityDialog
	bind:open={createOpen}
	kind="user"
	onOpenChange={(v) => (createOpen = v)}
	onCreated={load}
/>
