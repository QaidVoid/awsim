<script lang="ts">
	import { listNodegroupsWithDetail, type Cluster, type Nodegroup } from '$lib/api/eks';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import { toast } from 'svelte-sonner';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Cpu from '@lucide/svelte/icons/cpu';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		cluster: Cluster | null;
	}

	let { cluster }: Props = $props();

	let nodegroups = $state<Nodegroup[]>([]);
	let loading = $state(false);
	let lastName = $state('');

	$effect(() => {
		if (cluster && cluster.name !== lastName) {
			lastName = cluster.name;
			void load();
		}
		if (!cluster) {
			nodegroups = [];
			lastName = '';
		}
	});

	async function load() {
		if (!cluster) return;
		loading = true;
		try {
			const r = await listNodegroupsWithDetail(cluster.name);
			nodegroups = r.nodegroups;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load nodegroups');
		} finally {
			loading = false;
		}
	}

	function statusVariant(s: string): 'default' | 'secondary' | 'destructive' | 'outline' {
		if (s === 'ACTIVE') return 'default';
		if (s === 'CREATE_FAILED' || s === 'DEGRADED') return 'destructive';
		return 'secondary';
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{cluster ? `Nodegroups in ${cluster.name}` : 'Pick a cluster to inspect nodegroups'}
		</div>
		<Button type="button" variant="outline" size="sm" onclick={load} disabled={loading || !cluster}>
			<RefreshCw />
			Refresh
		</Button>
	</header>

	<div class="min-h-0 flex-1 overflow-y-auto">
		{#if !cluster}
			<div class="p-6">
				<EmptyState icon={Cpu} title="No cluster selected" description="Choose a cluster from the Clusters tab." />
			</div>
		{:else if loading && nodegroups.length === 0}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if nodegroups.length === 0}
			<div class="p-6">
				<EmptyState icon={Cpu} title="No nodegroups" description="This cluster has no nodegroups." />
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="border-b border-border bg-background/95 text-left text-muted-foreground">
					<tr>
						<th class="px-4 py-2 font-medium">Name</th>
						<th class="px-4 py-2 font-medium">Status</th>
						<th class="px-4 py-2 font-medium">Capacity</th>
						<th class="px-4 py-2 font-medium">Instances</th>
						<th class="px-4 py-2 text-right font-medium">Disk</th>
						<th class="px-4 py-2 text-right font-medium">Min</th>
						<th class="px-4 py-2 text-right font-medium">Desired</th>
						<th class="px-4 py-2 text-right font-medium">Max</th>
						<th class="px-4 py-2 font-medium">AMI</th>
					</tr>
				</thead>
				<tbody>
					{#each nodegroups as n (n.name)}
						<tr class="border-b border-border/40 hover:bg-muted/40">
							<td class="px-4 py-2 font-mono text-xs">{n.name}</td>
							<td class="px-4 py-2"><Badge variant={statusVariant(n.status)}>{n.status}</Badge></td>
							<td class="px-4 py-2 text-xs text-muted-foreground">{n.capacityType || '—'}</td>
							<td class="px-4 py-2 font-mono text-[11px]">{n.instanceTypes.join(', ') || '—'}</td>
							<td class="px-4 py-2 text-right">{n.diskSize} GB</td>
							<td class="px-4 py-2 text-right">{n.minSize}</td>
							<td class="px-4 py-2 text-right">{n.desiredSize}</td>
							<td class="px-4 py-2 text-right">{n.maxSize}</td>
							<td class="px-4 py-2 text-xs text-muted-foreground">{n.amiType || '—'}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>
