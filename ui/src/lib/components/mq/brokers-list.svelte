<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import MessagesSquareIcon from '@lucide/svelte/icons/messages-square';
	import { toast } from 'svelte-sonner';
	import { listBrokers, type BrokerSummary } from '$lib/api/mq';

	interface Props {
		onSelect: (b: BrokerSummary) => void;
		onCreate: () => void;
		refreshKey?: number;
	}

	let { onSelect, onCreate, refreshKey = 0 }: Props = $props();

	let rows = $state<BrokerSummary[]>([]);
	let loading = $state(false);

	$effect(() => {
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			rows = await listBrokers();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load brokers');
		} finally {
			loading = false;
		}
	}

	function stateColor(s: string): string {
		if (s === 'RUNNING') return 'text-green-500';
		if (s === 'CREATION_FAILED' || s === 'DELETION_FAILED') return 'text-destructive';
		return 'text-amber-500';
	}

	function timestamp(t: number): string {
		return new Date(t * 1000).toLocaleString();
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Brokers
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={onCreate}>
				<PlusIcon />
				Create broker
			</Button>
		</div>
	</div>

	<DataTable
		{rows}
		{loading}
		onRowClick={onSelect}
		columns={[
			{ key: 'brokerName', label: 'Name', mono: true },
			{ key: 'engineType', label: 'Engine', width: '110px', cell: engineCell },
			{ key: 'hostInstanceType', label: 'Instance', width: '160px', mono: true },
			{ key: 'deploymentMode', label: 'Deployment', width: '170px', mono: true },
			{ key: 'brokerState', label: 'State', width: '110px', cell: stateCell },
			{ key: 'created', label: 'Created', width: '180px', cell: createdCell }
		]}
		rowKey={(r) => r.brokerId}
	>
		{#snippet empty()}
			<EmptyState
				icon={MessagesSquareIcon}
				title="No brokers"
				description="Create an Amazon MQ broker (ActiveMQ or RabbitMQ) to back JMS / AMQP integrations."
			>
				{#snippet action()}
					<Button onclick={onCreate}>
						<PlusIcon />
						Create broker
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

{#snippet engineCell(row: BrokerSummary)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">{row.engineType}</Badge>
{/snippet}

{#snippet stateCell(row: BrokerSummary)}
	<Badge variant="outline" class={`h-5 px-2 text-[10px] ${stateColor(row.brokerState)}`}>
		{row.brokerState}
	</Badge>
{/snippet}

{#snippet createdCell(row: BrokerSummary)}
	<span class="text-xs">{timestamp(row.created)}</span>
{/snippet}
