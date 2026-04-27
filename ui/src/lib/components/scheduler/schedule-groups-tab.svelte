<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { DataTable, EmptyState } from '$lib/components/service';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import LayersIcon from '@lucide/svelte/icons/layers';
	import { toast } from 'svelte-sonner';
	import {
		listScheduleGroups,
		createScheduleGroup,
		deleteScheduleGroup,
		type ScheduleGroup,
	} from '$lib/api/scheduler';

	interface Props {
		onChange?: () => void;
	}

	let { onChange }: Props = $props();

	let groups = $state<ScheduleGroup[]>([]);
	let loading = $state(false);

	let createOpen = $state(false);
	let newName = $state('');
	let creating = $state(false);

	let confirmDelete = $state<string | null>(null);
	let deleting = $state(false);

	$effect(() => {
		void load();
	});

	async function load() {
		loading = true;
		try {
			groups = await listScheduleGroups();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load groups');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!newName.trim()) {
			toast.error('Group name is required.');
			return;
		}
		creating = true;
		try {
			await createScheduleGroup(newName.trim());
			toast.success('Schedule group created.');
			newName = '';
			createOpen = false;
			await load();
			onChange?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create group');
		} finally {
			creating = false;
		}
	}

	async function remove(name: string) {
		if (name === 'default') {
			toast.error('Cannot delete the default group.');
			confirmDelete = null;
			return;
		}
		deleting = true;
		try {
			await deleteScheduleGroup(name);
			toast.success('Schedule group deleted.');
			confirmDelete = null;
			await load();
			onChange?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		} finally {
			deleting = false;
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Schedule groups
			<span class="ml-1 font-normal text-muted-foreground">({groups.length})</span>
		</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={() => (createOpen = true)}>
				<PlusIcon />
				New group
			</Button>
		</div>
	</div>

	<DataTable
		rows={groups}
		{loading}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'state', label: 'State', width: '120px', cell: stateCell },
			{ key: 'arn', label: 'ARN', mono: true },
			{ key: 'actions', label: '', width: '60px', align: 'right', cell: actionsCell },
		]}
		rowKey={(g) => g.arn}
	>
		{#snippet empty()}
			<EmptyState
				icon={LayersIcon}
				title="No schedule groups"
				description="Groups organize related schedules. The default group always exists."
			>
				{#snippet action()}
					<Button onclick={() => (createOpen = true)}>
						<PlusIcon />
						Create group
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

{#snippet stateCell(row: ScheduleGroup)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] text-green-500">{row.state}</Badge>
{/snippet}

{#snippet actionsCell(row: ScheduleGroup)}
	{#if row.name !== 'default'}
		<Button
			variant="ghost"
			size="icon-xs"
			class="text-destructive hover:text-destructive"
			onclick={() => (confirmDelete = row.name)}
			aria-label="Delete group"
		>
			<Trash2Icon />
		</Button>
	{/if}
{/snippet}

<Dialog open={createOpen} onOpenChange={(o) => (createOpen = o)}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New schedule group</DialogTitle>
			<DialogDescription>
				A group is a namespace for schedules. Names must be unique per account/region.
			</DialogDescription>
		</DialogHeader>
		<div class="flex flex-col gap-2 px-4">
			<Label for="grp-name">Name</Label>
			<Input id="grp-name" bind:value={newName} placeholder="my-group" />
		</div>
		<DialogFooter>
			<Button variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
			<Button onclick={create} disabled={creating || !newName.trim()}>
				{creating ? 'Creating…' : 'Create group'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>

<Dialog
	open={confirmDelete !== null}
	onOpenChange={(o) => {
		if (!o) confirmDelete = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Delete schedule group?</DialogTitle>
			<DialogDescription>
				Removes <span class="font-mono">{confirmDelete}</span>. The group must be empty.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmDelete = null)}>Cancel</Button>
			<Button
				variant="destructive"
				disabled={deleting}
				onclick={() => confirmDelete && remove(confirmDelete)}
			>
				{deleting ? 'Deleting…' : 'Delete'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
