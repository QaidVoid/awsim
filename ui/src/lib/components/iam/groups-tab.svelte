<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { listGroups, type IamGroup } from '$lib/api/iam';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import CreateEntityDialog from './create-entity-dialog.svelte';
	import UsersRound from '@lucide/svelte/icons/users-round';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';

	let groups = $state<IamGroup[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let createOpen = $state(false);

	const filtered = $derived(
		filter.trim()
			? groups.filter((g) => g.groupName.toLowerCase().includes(filter.trim().toLowerCase()))
			: groups
	);

	async function load() {
		loading = true;
		try {
			groups = await listGroups();
		} finally {
			loading = false;
		}
	}

	function openDetail(g: IamGroup) {
		goto(`/iam/groups/${encodeURIComponent(g.groupName)}`);
	}

	onMount(load);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input type="search" placeholder="Filter groups..." bind:value={filter} class="h-8 max-w-xs" />
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {groups.length}</Badge>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<Plus class="size-3.5" />
			<span class="ml-1">New group</span>
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
				{ key: 'groupName', label: 'Group name', width: '30%' },
				{ key: 'arn', label: 'ARN', mono: true },
				{ key: 'groupId', label: 'Group ID', width: '20%', mono: true }
			]}
			rowKey={(r: IamGroup) => r.arn || r.groupName}
			onRowClick={openDetail}
		>
			{#snippet empty()}
				<EmptyState
					icon={UsersRound}
					title="No IAM groups"
					description="Groups make it easier to manage permissions for collections of users."
				/>
			{/snippet}
		</DataTable>
	</div>
</div>

<CreateEntityDialog
	bind:open={createOpen}
	kind="group"
	onOpenChange={(v) => (createOpen = v)}
	onCreated={load}
/>
