<script lang="ts">
	import type { Subnet } from '$lib/api/ec2';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import LayersIcon from '@lucide/svelte/icons/layers';

	interface Props {
		subnets: Subnet[];
		loading: boolean;
		onReload: () => void;
	}

	let { subnets, loading, onReload }: Props = $props();
</script>

<div class="flex h-full min-h-0 flex-col">
	<header
		class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2"
	>
		<div class="text-xs text-muted-foreground">
			{subnets.length} subnet{subnets.length === 1 ? '' : 's'}
		</div>
		<Button variant="outline" size="sm" onclick={onReload} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</header>

	<div class="min-h-0 flex-1">
		<DataTable
			rows={subnets}
			{loading}
			rowKey={(r) => r.subnetId}
			columns={[
				{ key: 'subnetId', label: 'Subnet ID', mono: true },
				{ key: 'vpcId', label: 'VPC', mono: true },
				{ key: 'cidrBlock', label: 'CIDR', mono: true },
				{ key: 'availabilityZone', label: 'AZ' },
				{ key: 'availableIpAddressCount', label: 'Available IPs', align: 'right' },
				{ key: 'mapPublicIpOnLaunch', label: 'Public IP', cell: publicCell },
				{ key: 'state', label: 'State', cell: stateCell }
			]}
		>
			{#snippet empty()}
				<EmptyState
					icon={LayersIcon}
					title="No subnets"
					description="Subnets partition a VPC into address ranges where instances launch. The default VPC provisions subnets automatically once it exists."
				/>
			{/snippet}
		</DataTable>
	</div>
</div>

{#snippet publicCell(row: Subnet)}
	<span class="text-xs text-muted-foreground">{row.mapPublicIpOnLaunch ? 'Yes' : 'No'}</span>
{/snippet}
{#snippet stateCell(row: Subnet)}
	<Badge variant={row.state === 'available' ? 'default' : 'outline'}>{row.state}</Badge>
{/snippet}
