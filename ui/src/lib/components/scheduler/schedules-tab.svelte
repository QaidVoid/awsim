<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
	import { toast } from 'svelte-sonner';
	import { listSchedules, type ScheduleSummary } from '$lib/api/scheduler';

	interface Props {
		groupName: string;
		onSelect: (s: ScheduleSummary) => void;
		onCreate: () => void;
		refreshKey?: number;
	}

	let { groupName, onSelect, onCreate, refreshKey = 0 }: Props = $props();

	let rows = $state<ScheduleSummary[]>([]);
	let loading = $state(false);

	$effect(() => {
		groupName;
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			rows = await listSchedules(groupName === 'ALL' ? undefined : groupName);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load schedules');
		} finally {
			loading = false;
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Schedules
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={onCreate}>
				<PlusIcon />
				New schedule
			</Button>
		</div>
	</div>

	<DataTable
		{rows}
		{loading}
		onRowClick={onSelect}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'groupName', label: 'Group', width: '160px' },
			{ key: 'scheduleExpression', label: 'Expression', mono: true },
			{ key: 'state', label: 'State', width: '110px', cell: stateCell },
		]}
		rowKey={(r) => r.arn}
	>
		{#snippet empty()}
			<EmptyState
				icon={CalendarClockIcon}
				title="No schedules"
				description="Create a schedule to fire targets on a rate or cron expression."
			>
				{#snippet action()}
					<Button onclick={onCreate}>
						<PlusIcon />
						Create schedule
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

{#snippet stateCell(row: ScheduleSummary)}
	<Badge
		variant="outline"
		class={row.state === 'ENABLED'
			? 'h-5 px-2 text-[10px] text-green-500'
			: 'h-5 px-2 text-[10px] text-muted-foreground'}
	>
		{row.state}
	</Badge>
{/snippet}
