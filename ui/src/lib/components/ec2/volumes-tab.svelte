<script lang="ts">
	import type { Volume } from '$lib/api/ec2';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import HardDriveIcon from '@lucide/svelte/icons/hard-drive';

	interface Props {
		volumes: Volume[];
		loading: boolean;
		onReload: () => void;
	}

	let { volumes, loading, onReload }: Props = $props();

	function stateVariant(s: string): 'default' | 'secondary' | 'outline' {
		if (s === 'in-use') return 'default';
		if (s === 'available') return 'secondary';
		return 'outline';
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header
		class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2"
	>
		<div class="text-xs text-muted-foreground">
			{volumes.length} volume{volumes.length === 1 ? '' : 's'}
		</div>
		<Button variant="outline" size="sm" onclick={onReload} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</header>

	<div class="min-h-0 flex-1">
		<DataTable
			rows={volumes}
			{loading}
			rowKey={(r) => r.volumeId}
			columns={[
				{ key: 'volumeId', label: 'Volume ID', mono: true },
				{ key: 'size', label: 'Size', align: 'right', cell: sizeCell },
				{ key: 'volumeType', label: 'Type' },
				{ key: 'state', label: 'State', cell: stateCell },
				{ key: 'availabilityZone', label: 'AZ' },
				{ key: 'attachments', label: 'Attached to', cell: attachCell },
				{ key: 'encrypted', label: 'Encrypted', cell: encCell }
			]}
		>
			{#snippet empty()}
				<EmptyState
					icon={HardDriveIcon}
					title="No volumes"
					description="EBS volumes appear here once you create or attach them to instances."
				/>
			{/snippet}
		</DataTable>
	</div>
</div>

{#snippet sizeCell(row: Volume)}
	<span class="text-xs">{row.size} GiB</span>
{/snippet}
{#snippet stateCell(row: Volume)}
	<Badge variant={stateVariant(row.state)}>{row.state}</Badge>
{/snippet}
{#snippet attachCell(row: Volume)}
	{#if row.attachments.length === 0}
		<span class="text-xs text-muted-foreground">—</span>
	{:else}
		<span class="font-mono text-xs">{row.attachments[0].instanceId} ({row.attachments[0].device})</span>
	{/if}
{/snippet}
{#snippet encCell(row: Volume)}
	<span class="text-xs text-muted-foreground">{row.encrypted ? 'Yes' : 'No'}</span>
{/snippet}
