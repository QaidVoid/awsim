<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
	import { toast } from 'svelte-sonner';
	import { listPlans, createPlan, deletePlan, type BackupPlan } from '$lib/api/backup';

	interface Props {
		refreshKey?: number;
		onChanged?: () => void;
	}

	let { refreshKey = 0, onChanged }: Props = $props();

	let rows = $state<BackupPlan[]>([]);
	let loading = $state(false);
	let newName = $state('');
	let newVault = $state('');
	let creating = $state(false);

	$effect(() => {
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			rows = await listPlans();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load plans');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!newName.trim() || !newVault.trim()) {
			return toast.error('Plan name and target vault are required.');
		}
		creating = true;
		try {
			await createPlan(newName.trim(), newVault.trim());
			toast.success(`Created plan "${newName.trim()}".`);
			newName = '';
			newVault = '';
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create plan');
		} finally {
			creating = false;
		}
	}

	async function remove(p: BackupPlan) {
		if (!confirm(`Delete plan "${p.planName}"?`)) return;
		try {
			await deletePlan(p.planId);
			toast.success('Plan deleted.');
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete plan');
		}
	}

	function timestamp(t?: number): string {
		if (!t) return '—';
		return new Date(t * 1000).toLocaleString();
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Backup plans
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="flex flex-wrap items-center gap-2">
		<Input bind:value={newName} placeholder="plan name" class="h-8 max-w-[200px]" />
		<Input bind:value={newVault} placeholder="target vault" class="h-8 max-w-[200px]" />
		<Button size="sm" onclick={create} disabled={creating}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create plan'}
		</Button>
	</div>
	<p class="text-[11px] text-muted-foreground">
		Default rule: <code>cron(0 5 ? * * *)</code> with 30-day delete-after lifecycle.
	</p>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'planName', label: 'Name', mono: true },
			{ key: 'planId', label: 'ID', mono: true },
			{ key: 'versionId', label: 'Version', width: '150px', mono: true },
			{ key: 'lastExecutionDate', label: 'Last exec', width: '180px', cell: lastCell },
			{ key: 'creationDate', label: 'Created', width: '180px', cell: createdCell },
			{ key: '__actions', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.planId}
	>
		{#snippet empty()}
			<EmptyState
				icon={CalendarClockIcon}
				title="No backup plans"
				description="A backup plan defines the schedule, vault, and retention for backups."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet lastCell(row: BackupPlan)}
	<span class="text-xs">{timestamp(row.lastExecutionDate)}</span>
{/snippet}

{#snippet createdCell(row: BackupPlan)}
	<span class="text-xs">{timestamp(row.creationDate)}</span>
{/snippet}

{#snippet actionsCell(row: BackupPlan)}
	<Button variant="ghost" size="xs" onclick={() => remove(row)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}
