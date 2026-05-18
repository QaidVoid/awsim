<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import MegaphoneIcon from '@lucide/svelte/icons/megaphone';
	import { toast } from 'svelte-sonner';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import {
		listSegments,
		listCampaigns,
		createCampaign,
		deleteCampaign,
		type Campaign,
		type Segment
	} from '$lib/api/pinpoint';

	interface Props {
		appId: string;
		refreshKey?: number;
	}

	let { appId, refreshKey = 0 }: Props = $props();

	let rows = $state<Campaign[]>([]);
	let segments = $state<Segment[]>([]);
	let loading = $state(false);
	let newName = $state('');
	let newSegment = $state('');
	let creating = $state(false);
	let deleteTarget = $state<Campaign | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	$effect(() => {
		appId;
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			[rows, segments] = await Promise.all([listCampaigns(appId), listSegments(appId)]);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load campaigns');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!newName.trim() || !newSegment) return toast.error('Pick a name and segment.');
		creating = true;
		try {
			await createCampaign({
				appId,
				name: newName.trim(),
				segmentId: newSegment
			});
			toast.success(`Created campaign "${newName.trim()}".`);
			newName = '';
			newSegment = '';
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create campaign');
		} finally {
			creating = false;
		}
	}

	function remove(c: Campaign) {
		deleteTarget = c;
		deleteOpen = true;
	}

	async function confirmRemove() {
		if (!deleteTarget) return;
		deleteBusy = true;
		try {
			await deleteCampaign(appId, deleteTarget.id);
			toast.success('Campaign deleted.');
			deleteOpen = false;
			deleteTarget = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		} finally {
			deleteBusy = false;
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Campaigns
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="flex flex-wrap items-center gap-2">
		<Input bind:value={newName} placeholder="campaign name" class="h-8 max-w-[200px] text-xs" />
		<select
			bind:value={newSegment}
			class="h-8 rounded-md border border-border bg-background px-2 text-xs"
		>
			<option value="">Pick segment…</option>
			{#each segments as s (s.id)}
				<option value={s.id}>{s.name}</option>
			{/each}
		</select>
		<Button size="sm" onclick={create} disabled={creating || segments.length === 0}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create campaign'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'id', label: 'ID', mono: true, width: '280px' },
			{ key: 'segmentId', label: 'Segment', mono: true, width: '280px' },
			{ key: 'state', label: 'State', width: '120px', cell: stateCell },
			{ key: '__actions', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.id}
	>
		{#snippet empty()}
			<EmptyState
				icon={MegaphoneIcon}
				title="No campaigns"
				description="A campaign delivers a message to a segment. AWSim collapses delivery — campaigns land in COMPLETED on Create."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet stateCell(row: Campaign)}
	<Badge
		variant="outline"
		class={row.state === 'COMPLETED'
			? 'h-5 px-2 text-[10px] text-green-500'
			: 'h-5 px-2 text-[10px] text-amber-500'}
	>
		{row.state}
	</Badge>
{/snippet}

{#snippet actionsCell(row: Campaign)}
	<Button variant="ghost" size="xs" onclick={() => remove(row)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete campaign?"
	description={`Delete campaign "${deleteTarget?.name ?? ''}".`}
	busy={deleteBusy}
	onConfirm={confirmRemove}
	onClose={() => {
		deleteOpen = false;
		deleteTarget = null;
	}}
/>
