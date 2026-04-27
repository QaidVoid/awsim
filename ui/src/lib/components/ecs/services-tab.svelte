<script lang="ts">
	import { listServices, type Cluster, type ServiceSummary } from '$lib/api/ecs';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import { toast } from 'svelte-sonner';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Network from '@lucide/svelte/icons/network';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		cluster: Cluster | null;
	}

	let { cluster }: Props = $props();

	let services = $state<ServiceSummary[]>([]);
	let loading = $state(false);
	let lastArn = $state('');

	$effect(() => {
		if (cluster && cluster.arn !== lastArn) {
			lastArn = cluster.arn;
			void load();
		}
		if (!cluster) {
			services = [];
			lastArn = '';
		}
	});

	async function load() {
		if (!cluster) return;
		loading = true;
		try {
			const r = await listServices(cluster.arn);
			services = r.services;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load services');
		} finally {
			loading = false;
		}
	}

	function statusVariant(s: string): 'default' | 'secondary' | 'destructive' {
		if (s === 'ACTIVE') return 'default';
		if (s === 'DRAINING') return 'secondary';
		return 'destructive';
	}

	function shortArn(arn: string): string {
		return arn.split('/').pop() ?? arn;
	}

	function formatDate(iso: string): string {
		if (!iso) return '—';
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{cluster ? `Services in ${cluster.name}` : 'Pick a cluster from the Clusters tab'}
		</div>
		<Button type="button" variant="outline" size="sm" onclick={load} disabled={loading || !cluster}>
			<RefreshCw />
			Refresh
		</Button>
	</header>

	<div class="min-h-0 flex-1 overflow-y-auto">
		{#if !cluster}
			<div class="p-6">
				<EmptyState
					icon={Network}
					title="No cluster selected"
					description="Choose a cluster on the Clusters tab to inspect services."
				/>
			</div>
		{:else if loading && services.length === 0}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if services.length === 0}
			<div class="p-6">
				<EmptyState icon={Network} title="No services" description="This cluster has no services." />
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="border-b border-border bg-background/95 text-left text-muted-foreground">
					<tr>
						<th class="px-4 py-2 font-medium">Name</th>
						<th class="px-4 py-2 font-medium">Status</th>
						<th class="px-4 py-2 text-right font-medium">Desired</th>
						<th class="px-4 py-2 text-right font-medium">Running</th>
						<th class="px-4 py-2 text-right font-medium">Pending</th>
						<th class="px-4 py-2 font-medium">Task definition</th>
						<th class="px-4 py-2 font-medium">Launch</th>
						<th class="px-4 py-2 font-medium">Created</th>
					</tr>
				</thead>
				<tbody>
					{#each services as svc (svc.arn)}
						<tr class="border-b border-border/40 hover:bg-muted/40">
							<td class="px-4 py-2 font-mono text-xs">{svc.name}</td>
							<td class="px-4 py-2">
								<Badge variant={statusVariant(svc.status)}>{svc.status}</Badge>
							</td>
							<td class="px-4 py-2 text-right">{svc.desiredCount}</td>
							<td class="px-4 py-2 text-right">{svc.runningCount}</td>
							<td class="px-4 py-2 text-right">{svc.pendingCount}</td>
							<td class="truncate px-4 py-2 font-mono text-[11px]">{shortArn(svc.taskDefinition)}</td>
							<td class="px-4 py-2 text-xs text-muted-foreground">{svc.launchType || '—'}</td>
							<td class="px-4 py-2 text-xs text-muted-foreground">{formatDate(svc.createdAt)}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>
