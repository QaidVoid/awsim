<script lang="ts">
	import { onMount } from 'svelte';
	import { listGroups, getGroup, type IamGroup, type IamUser } from '$lib/api/iam';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import EntityDetailSheet from './entity-detail-sheet.svelte';
	import UsersRound from '@lucide/svelte/icons/users-round';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';

	let groups = $state<IamGroup[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let selected = $state<IamGroup | null>(null);
	let members = $state<IamUser[]>([]);
	let detailLoading = $state(false);

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

	async function openDetail(g: IamGroup) {
		selected = g;
		members = [];
		detailLoading = true;
		try {
			const detail = await getGroup(g.groupName);
			selected = detail.group;
			members = detail.users;
		} finally {
			detailLoading = false;
		}
	}

	onMount(load);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input type="search" placeholder="Filter groups..." bind:value={filter} class="h-8 max-w-xs" />
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {groups.length}</Badge>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
	</div>

	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
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

<EntityDetailSheet
	open={!!selected}
	onOpenChange={(v) => {
		if (!v) selected = null;
	}}
	title={selected?.groupName ?? ''}
	subtitle={selected?.arn}
>
	{#if selected}
		<dl class="grid grid-cols-3 gap-x-4 gap-y-2 py-4 text-sm">
			<dt class="text-muted-foreground">Group ID</dt>
			<dd class="col-span-2 font-mono text-xs">{selected.groupId}</dd>
			<dt class="text-muted-foreground">ARN</dt>
			<dd class="col-span-2 break-all font-mono text-xs">{selected.arn}</dd>
		</dl>
		<div class="mt-4">
			<h3 class="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
				Members
			</h3>
			{#if detailLoading}
				<p class="text-xs text-muted-foreground">Loading members...</p>
			{:else if members.length === 0}
				<p class="text-xs text-muted-foreground">No users in this group.</p>
			{:else}
				<ul class="space-y-1.5">
					{#each members as m (m.arn)}
						<li class="rounded border border-border/60 px-3 py-2 text-sm">
							<div class="font-medium">{m.userName}</div>
							<div class="truncate font-mono text-xs text-muted-foreground">{m.arn}</div>
						</li>
					{/each}
				</ul>
			{/if}
		</div>
	{/if}
</EntityDetailSheet>
