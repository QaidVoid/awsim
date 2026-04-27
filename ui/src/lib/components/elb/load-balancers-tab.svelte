<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import NetworkIcon from '@lucide/svelte/icons/network';
	import type { LoadBalancer } from '$lib/api/elb';

	interface Props {
		loadBalancers: LoadBalancer[];
		loading: boolean;
		onRefresh: () => void;
		onSelect: (lb: LoadBalancer) => void;
		onCreate: () => void;
	}

	let { loadBalancers, loading, onRefresh, onSelect, onCreate }: Props = $props();
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Load balancers
			<span class="ml-1 font-normal text-muted-foreground">({loadBalancers.length})</span>
		</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={onRefresh} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={onCreate}>
				<PlusIcon />
				New load balancer
			</Button>
		</div>
	</div>

	<DataTable
		rows={loadBalancers}
		{loading}
		onRowClick={onSelect}
		rowKey={(lb) => lb.arn}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'type', label: 'Type', width: '120px', cell: typeCell },
			{ key: 'scheme', label: 'Scheme', width: '140px' },
			{ key: 'dnsName', label: 'DNS', mono: true, cell: dnsCell },
			{ key: 'state', label: 'State', width: '100px', cell: stateCell },
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={NetworkIcon}
				title="No load balancers"
				description="Create an Application, Network, or Gateway load balancer to distribute traffic."
			>
				{#snippet action()}
					<Button onclick={onCreate}>
						<PlusIcon />
						Create load balancer
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

{#snippet typeCell(lb: LoadBalancer)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] uppercase">{lb.type}</Badge>
{/snippet}

{#snippet dnsCell(lb: LoadBalancer)}
	<span class="font-mono text-[11px] text-muted-foreground">{lb.dnsName || '—'}</span>
{/snippet}

{#snippet stateCell(lb: LoadBalancer)}
	<Badge
		variant="outline"
		class={lb.state === 'active'
			? 'h-5 px-2 text-[10px] text-green-500'
			: 'h-5 px-2 text-[10px] text-muted-foreground'}
	>
		{lb.state}
	</Badge>
{/snippet}
