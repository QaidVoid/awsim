<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import TargetIcon from '@lucide/svelte/icons/target';
	import type { TargetGroup } from '$lib/api/elb';

	interface Props {
		targetGroups: TargetGroup[];
		loading: boolean;
		onRefresh: () => void;
		onSelect: (tg: TargetGroup) => void;
		onCreate: () => void;
	}

	let { targetGroups, loading, onRefresh, onSelect, onCreate }: Props = $props();
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Target groups
			<span class="ml-1 font-normal text-muted-foreground">({targetGroups.length})</span>
		</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={onRefresh} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={onCreate}>
				<PlusIcon />
				New target group
			</Button>
		</div>
	</div>

	<DataTable
		rows={targetGroups}
		{loading}
		onRowClick={onSelect}
		rowKey={(tg) => tg.arn}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'protocol', label: 'Protocol', width: '110px' },
			{ key: 'port', label: 'Port', width: '80px', align: 'right' },
			{ key: 'targetType', label: 'Target type', width: '130px', cell: typeCell },
			{ key: 'healthCheckPath', label: 'Health check', cell: hcCell },
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={TargetIcon}
				title="No target groups"
				description="Target groups route traffic to instances, IPs, Lambda functions, or other ALBs."
			>
				{#snippet action()}
					<Button onclick={onCreate}>
						<PlusIcon />
						Create target group
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

{#snippet typeCell(tg: TargetGroup)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] uppercase">{tg.targetType}</Badge>
{/snippet}

{#snippet hcCell(tg: TargetGroup)}
	<span class="font-mono text-[11px] text-muted-foreground">
		{tg.healthCheckProtocol || '—'} {tg.healthCheckPath || ''}
	</span>
{/snippet}
