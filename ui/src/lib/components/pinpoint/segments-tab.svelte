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
	import {
		listSegments,
		createSegment,
		deleteSegment,
		type Segment
	} from '$lib/api/pinpoint';

	interface Props {
		appId: string;
		refreshKey?: number;
	}

	let { appId, refreshKey = 0 }: Props = $props();

	let rows = $state<Segment[]>([]);
	let loading = $state(false);
	let newName = $state('');
	let creating = $state(false);

	$effect(() => {
		appId;
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			rows = await listSegments(appId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load segments');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!newName.trim()) return toast.error('Segment name is required.');
		creating = true;
		try {
			await createSegment(appId, newName.trim());
			toast.success(`Created segment "${newName.trim()}".`);
			newName = '';
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create segment');
		} finally {
			creating = false;
		}
	}

	async function remove(s: Segment) {
		if (!confirm(`Delete segment "${s.name}"?`)) return;
		try {
			await deleteSegment(appId, s.id);
			toast.success('Segment deleted.');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Segments
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="flex items-center gap-2">
		<Input bind:value={newName} placeholder="segment name" class="h-8 max-w-[260px] text-xs" />
		<Button size="sm" onclick={create} disabled={creating}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create segment'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'id', label: 'ID', mono: true, width: '300px' },
			{ key: 'segmentType', label: 'Type', width: '130px', cell: typeCell },
			{ key: 'version', label: 'Ver', width: '60px' },
			{ key: '__actions', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.id}
	>
		{#snippet empty()}
			<EmptyState icon={UsersIcon} title="No segments" description="Create a segment to target with a campaign." />
		{/snippet}
	</DataTable>
</div>

{#snippet typeCell(row: Segment)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">{row.segmentType}</Badge>
{/snippet}

{#snippet actionsCell(row: Segment)}
	<Button variant="ghost" size="xs" onclick={() => remove(row)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}
