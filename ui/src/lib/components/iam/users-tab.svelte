<script lang="ts">
	import { onMount } from 'svelte';
	import { pendingAction } from '$lib/pending-action.svelte';
	import { goto } from '$app/navigation';
	import { route } from '$lib/url';
	import { listUsers, type IamUser } from '$lib/api/iam';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import CreateEntityDialog from './create-entity-dialog.svelte';
	import Users from '@lucide/svelte/icons/users';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';

	let users = $state<IamUser[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let createOpen = $state(false);

	onMount(() => {
		if (pendingAction.consume('new-user')) createOpen = true;
	});

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

	function openDetail(u: IamUser) {
		goto(route(`/iam/users/${encodeURIComponent(u.userName)}`));
	}

	onMount(load);
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

<CreateEntityDialog
	bind:open={createOpen}
	kind="user"
	onOpenChange={(v) => (createOpen = v)}
	onCreated={load}
/>
