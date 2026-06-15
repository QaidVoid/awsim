<script lang="ts">
	import type { DBCluster } from '$lib/api/rds';
	import { statusVariant } from '$lib/api/rds';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { EmptyState } from '$lib/components/service';
	import ClusterDetail from '$lib/components/rds/cluster-detail.svelte';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Boxes from '@lucide/svelte/icons/boxes';
	import Search from '@lucide/svelte/icons/search';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';

	interface Props {
		clusters: DBCluster[];
		loading: boolean;
		onRefresh: () => void;
		onDeleteCluster: (cluster: DBCluster) => void;
		onChanged: () => Promise<void>;
	}

	let { clusters, loading, onRefresh, onDeleteCluster, onChanged }: Props = $props();

	let filter = $state('');
	let expandedId = $state<string | null>(null);

	let visible = $derived(
		filter.trim().length === 0
			? clusters
			: clusters.filter((c) =>
					c.identifier.toLowerCase().includes(filter.trim().toLowerCase())
				)
	);

	function toggle(id: string) {
		expandedId = expandedId === id ? null : id;
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div
		class="flex shrink-0 items-center gap-2 border-b border-border bg-background/40 px-4 py-2"
	>
		<div class="relative flex-1">
			<Search
				class="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground"
			/>
			<input
				type="text"
				bind:value={filter}
				placeholder="Filter clusters..."
				class="h-8 w-full rounded-md border border-border bg-background pr-2 pl-7 text-xs outline-none focus:border-ring focus:ring-1 focus:ring-ring"
			/>
		</div>
		<Button variant="ghost" size="icon-sm" onclick={onRefresh} aria-label="Refresh">
			{#if loading}
				<Loader2 class="size-3.5 animate-spin" />
			{:else}
				<RefreshCw class="size-3.5" />
			{/if}
		</Button>
	</div>

	<div class="min-h-0 flex-1 overflow-hidden">
		{#if loading && clusters.length === 0}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if visible.length === 0}
			<div class="flex h-full items-center justify-center p-6">
				<EmptyState
					icon={Boxes}
					title={filter ? 'No matches' : 'No clusters yet'}
					description={filter
						? 'Try a different filter.'
						: 'Create your first Aurora cluster.'}
				/>
			</div>
		{:else}
			<div class="h-full overflow-auto">
				<table class="w-full text-xs">
					<thead
						class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm"
					>
						<tr>
							<th class="w-6"></th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">
								Identifier
							</th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">Engine</th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">Status</th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">Members</th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">
								Writer endpoint
							</th>
						</tr>
					</thead>
					<tbody>
						{#each visible as cluster (cluster.identifier)}
							<tr
								class="cursor-pointer border-b border-border/40 transition-colors hover:bg-muted/40 {expandedId ===
								cluster.identifier
									? 'bg-muted/60'
									: ''}"
								onclick={() => toggle(cluster.identifier)}
							>
								<td class="pl-2 text-muted-foreground">
									<ChevronRight
										class="size-3.5 transition-transform {expandedId === cluster.identifier
											? 'rotate-90'
											: ''}"
									/>
								</td>
								<td class="px-3 py-2 font-mono">{cluster.identifier}</td>
								<td class="px-3 py-2 font-mono">
									{cluster.engine}{cluster.engineVersion ? ` ${cluster.engineVersion}` : ''}
								</td>
								<td class="px-3 py-2">
									<Badge variant={statusVariant(cluster.status)}>
										{cluster.status || 'unknown'}
									</Badge>
								</td>
								<td class="px-3 py-2 font-mono text-muted-foreground">
									{cluster.members.length}
								</td>
								<td class="px-3 py-2 font-mono text-muted-foreground">
									{cluster.endpoint || '—'}{cluster.port ? `:${cluster.port}` : ''}
								</td>
							</tr>
							{#if expandedId === cluster.identifier}
								<tr>
									<td colspan="6" class="p-0">
										<ClusterDetail {cluster} {onDeleteCluster} {onChanged} />
									</td>
								</tr>
							{/if}
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>
</div>
