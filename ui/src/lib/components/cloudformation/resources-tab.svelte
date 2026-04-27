<script lang="ts">
	import type { StackResource } from '$lib/api/cloudformation';
	import { stackStatusVariant } from '$lib/api/cloudformation';
	import { Badge } from '$lib/components/ui/badge';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import Boxes from '@lucide/svelte/icons/boxes';

	interface Props {
		resources: StackResource[];
		loading: boolean;
	}

	let { resources, loading }: Props = $props();
</script>

<div class="min-h-0 flex-1 overflow-auto">
	{#if loading && resources.length === 0}
		<div class="space-y-2 p-4">
			{#each Array(4) as _, i (i)}
				<Skeleton class="h-7 w-full" />
			{/each}
		</div>
	{:else if resources.length === 0}
		<div class="p-6">
			<EmptyState icon={Boxes} title="No resources" description="No resources yet." />
		</div>
	{:else}
		<table class="w-full text-sm">
			<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
				<tr>
					<th class="px-4 py-2 text-left font-medium text-muted-foreground">Logical ID</th>
					<th class="px-4 py-2 text-left font-medium text-muted-foreground">Physical ID</th>
					<th class="px-4 py-2 text-left font-medium text-muted-foreground">Type</th>
					<th class="px-4 py-2 text-left font-medium text-muted-foreground">Status</th>
				</tr>
			</thead>
			<tbody>
				{#each resources as r (r.logicalResourceId)}
					<tr class="border-b border-border/40 hover:bg-muted/30">
						<td class="px-4 py-2 font-mono text-xs">{r.logicalResourceId}</td>
						<td class="max-w-xs truncate px-4 py-2 font-mono text-[11px] text-muted-foreground">
							{r.physicalResourceId}
						</td>
						<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">
							{r.resourceType}
						</td>
						<td class="px-4 py-2">
							<Badge variant={stackStatusVariant(r.resourceStatus)}>{r.resourceStatus}</Badge>
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	{/if}
</div>
