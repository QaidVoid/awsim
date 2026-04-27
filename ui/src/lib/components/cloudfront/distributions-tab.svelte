<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import GlobeIcon from '@lucide/svelte/icons/globe';
	import type { Distribution } from '$lib/api/cloudfront';

	interface Props {
		distributions: Distribution[];
		loading: boolean;
		onRefresh: () => void;
		onSelect: (d: Distribution) => void;
		onCreate: () => void;
	}

	let { distributions, loading, onRefresh, onSelect, onCreate }: Props = $props();
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Distributions
			<span class="ml-1 font-normal text-muted-foreground">({distributions.length})</span>
		</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={onRefresh} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={onCreate}>
				<PlusIcon />
				New distribution
			</Button>
		</div>
	</div>

	<DataTable
		rows={distributions}
		{loading}
		onRowClick={onSelect}
		rowKey={(d) => d.id}
		columns={[
			{ key: 'id', label: 'ID', mono: true, width: '180px' },
			{ key: 'domainName', label: 'Domain', mono: true, cell: domainCell },
			{ key: 'origin', label: 'Origin', mono: true, cell: originCell },
			{ key: 'enabled', label: 'Enabled', width: '100px', cell: enabledCell },
			{ key: 'status', label: 'Status', width: '120px', cell: statusCell },
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={GlobeIcon}
				title="No CloudFront distributions"
				description="Distributions cache content at edge locations close to viewers."
			>
				{#snippet action()}
					<Button onclick={onCreate}>
						<PlusIcon />
						Create distribution
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

{#snippet domainCell(d: Distribution)}
	<span class="font-mono text-[11px]">{d.domainName || '—'}</span>
{/snippet}

{#snippet originCell(d: Distribution)}
	<span class="font-mono text-[11px] text-muted-foreground">{d.originDomainName || '—'}</span>
{/snippet}

{#snippet enabledCell(d: Distribution)}
	{#if d.enabled}
		<Badge variant="outline" class="h-5 px-2 text-[10px] text-green-500">enabled</Badge>
	{:else}
		<Badge variant="outline" class="h-5 px-2 text-[10px] text-muted-foreground">disabled</Badge>
	{/if}
{/snippet}

{#snippet statusCell(d: Distribution)}
	<Badge variant="outline" class="h-5 px-2 text-[10px]">{d.status || '—'}</Badge>
{/snippet}
