<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import { toast } from 'svelte-sonner';
	import {
		listEnvironments,
		createEnvironment,
		deleteEnvironment,
		type Environment
	} from '$lib/api/appconfig';

	interface Props {
		appId: string;
		refreshKey?: number;
	}

	let { appId, refreshKey = 0 }: Props = $props();

	let rows = $state<Environment[]>([]);
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
			rows = await listEnvironments(appId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load environments');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!newName.trim()) return toast.error('Environment name is required.');
		creating = true;
		try {
			await createEnvironment(appId, newName.trim());
			toast.success(`Created environment "${newName.trim()}".`);
			newName = '';
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create environment');
		} finally {
			creating = false;
		}
	}

	async function remove(e: Environment) {
		if (!confirm(`Delete environment "${e.name}"?`)) return;
		try {
			await deleteEnvironment(appId, e.id);
			toast.success('Environment deleted.');
			await load();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to delete');
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Environments
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="flex items-center gap-2">
		<Input bind:value={newName} placeholder="env name (prod, staging, …)" class="h-8 max-w-[260px]" />
		<Button size="sm" onclick={create} disabled={creating}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create environment'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'id', label: 'ID', mono: true, width: '120px' },
			{ key: 'state', label: 'State', width: '180px', cell: stateCell },
			{ key: 'id', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.id}
	>
		{#snippet empty()}
			<EmptyState
				icon={GitBranchIcon}
				title="No environments"
				description="Create an environment (typically prod / staging / dev) to deploy configuration into."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet stateCell(row: Environment)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">{row.state}</Badge>
{/snippet}

{#snippet actionsCell(row: Environment)}
	<Button variant="ghost" size="xs" onclick={() => remove(row)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}
