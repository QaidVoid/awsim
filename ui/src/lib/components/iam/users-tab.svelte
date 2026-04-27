<script lang="ts">
	import { onMount } from 'svelte';
	import { listUsers, getUser, type IamUser } from '$lib/api/iam';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import EntityDetailSheet from './entity-detail-sheet.svelte';
	import Users from '@lucide/svelte/icons/users';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';

	let users = $state<IamUser[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let selected = $state<IamUser | null>(null);
	let detailLoading = $state(false);

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
		try {
			selected = await getUser(u.userName);
		} finally {
			detailLoading = false;
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
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
	</div>

	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
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
	{/if}
</EntityDetailSheet>
