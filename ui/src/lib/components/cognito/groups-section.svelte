<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { listGroups, deleteGroup, type CognitoGroup } from '$lib/api/cognito';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { EmptyState, ListSkeleton } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import UsersRound from '@lucide/svelte/icons/users-round';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import GroupDetail from './group-detail.svelte';
	import CreateGroupDialog from './create-group-dialog.svelte';
	import EditGroupDialog from './edit-group-dialog.svelte';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';

	interface Props {
		poolId: string;
	}

	let { poolId }: Props = $props();

	let groups = $state<CognitoGroup[]>([]);
	let loading = $state(false);
	let expanded = $state<string | null>(null);
	let createOpen = $state(false);
	let editTarget = $state<CognitoGroup | null>(null);
	let editOpen = $state(false);
	let deleteName = $state<string | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	onMount(load);

	async function load() {
		loading = true;
		try {
			groups = await listGroups(poolId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load groups');
		} finally {
			loading = false;
		}
	}

	function openDelete(name: string) {
		deleteName = name;
		deleteOpen = true;
	}

	function openEdit(g: CognitoGroup) {
		editTarget = g;
		editOpen = true;
	}

	async function confirmDelete() {
		if (!deleteName) return;
		deleteBusy = true;
		try {
			await deleteGroup(poolId, deleteName);
			toast.success(`Deleted ${deleteName}`);
			deleteOpen = false;
			deleteName = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div
		class="sticky top-0 z-10 flex flex-wrap items-center gap-2 border-b border-border bg-background px-6 py-3"
	>
		<Badge variant="secondary">{groups.length} groups</Badge>
		<div class="flex-1"></div>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
		<Button size="xs" onclick={() => (createOpen = true)}>
			<Plus class="size-3.5" /> Group
		</Button>
	</div>

	<div class="flex-1 overflow-y-auto px-6 py-4">
		{#if loading && groups.length === 0}
			<ListSkeleton rows={4} />
		{:else if groups.length === 0}
			<EmptyState
				icon={UsersRound}
				title="No groups yet"
				description="Groups assign an IAM role and precedence to a set of users."
			>
				{#snippet action()}
					<Button size="sm" onclick={() => (createOpen = true)}>
						<Plus class="size-3.5" /> Create group
					</Button>
				{/snippet}
			</EmptyState>
		{:else}
			<ul class="space-y-1.5">
				{#each groups as g (g.name)}
					<li class="rounded border border-border/60">
						<div class="flex flex-wrap items-center gap-2 px-3 py-2 text-sm">
							<button
								type="button"
								class="flex min-w-0 flex-1 items-center gap-1.5 text-left"
								onclick={() => (expanded = expanded === g.name ? null : g.name)}
								aria-expanded={expanded === g.name}
							>
								<ChevronRight
									class="size-3.5 shrink-0 text-muted-foreground transition-transform {expanded ===
									g.name
										? 'rotate-90'
										: ''}"
								/>
								<div class="min-w-0">
									<div class="flex flex-wrap items-center gap-2 font-medium">
										{g.name}
										{#if g.precedence !== undefined}
											<Badge variant="outline" class="font-mono text-[10px]">
												prec {g.precedence}
											</Badge>
										{/if}
									</div>
									{#if g.description}
										<div class="text-xs text-muted-foreground">{g.description}</div>
									{/if}
									{#if g.roleArn}
										<div class="truncate font-mono text-xs text-muted-foreground">{g.roleArn}</div>
									{/if}
								</div>
							</button>
							<Button variant="ghost" size="xs" onclick={() => openEdit(g)}>
								Edit
							</Button>
							<Button
								variant="ghost"
								size="xs"
								class="text-destructive hover:text-destructive"
								onclick={() => openDelete(g.name)}
							>
								Delete
							</Button>
						</div>
						{#if expanded === g.name}
							<div class="border-t border-border/60 px-3 py-3">
								{#key g.name}
									<GroupDetail {poolId} groupName={g.name} />
								{/key}
							</div>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>

<CreateGroupDialog
	bind:open={createOpen}
	{poolId}
	onClose={() => (createOpen = false)}
	onCreated={() => void load()}
/>
<EditGroupDialog
	bind:open={editOpen}
	{poolId}
	group={editTarget}
	onClose={() => {
		editOpen = false;
		editTarget = null;
	}}
	onUpdated={() => void load()}
/>
{#if deleteName}
	<ConfirmDialog
		bind:open={deleteOpen}
		title="Delete group"
		description={`Delete group ${deleteName}? Members are not deleted but lose this membership.`}
		busy={deleteBusy}
		onConfirm={confirmDelete}
		onClose={() => {
			deleteOpen = false;
			deleteName = null;
		}}
	/>
{/if}
