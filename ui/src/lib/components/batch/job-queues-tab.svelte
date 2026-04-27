<script lang="ts">
	import { onMount } from 'svelte';
	import { describeJobQueues, jobStatusVariant, type JobQueue } from '$lib/api/batch';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import ListOrdered from '@lucide/svelte/icons/list-ordered';
	import { toast } from 'svelte-sonner';

	let queues = $state<JobQueue[]>([]);
	let loading = $state(true);

	async function reload() {
		loading = true;
		try {
			const r = await describeJobQueues();
			queues = r.jobQueues;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load job queues');
		} finally {
			loading = false;
		}
	}

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{queues.length} queue{queues.length === 1 ? '' : 's'}
		</div>
		<Button type="button" variant="outline" size="sm" onclick={reload} disabled={loading}>
			<RefreshCw class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</header>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if loading && queues.length === 0}
			<div class="space-y-2 p-4">
				{#each Array(4) as _, i (i)}
					<Skeleton class="h-7 w-full" />
				{/each}
			</div>
		{:else if queues.length === 0}
			<div class="p-6">
				<EmptyState icon={ListOrdered} title="No job queues" description="No job queues defined." />
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Name</th>
						<th class="px-4 py-2 text-right font-medium text-muted-foreground">Priority</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">State</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Status</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Compute envs</th>
					</tr>
				</thead>
				<tbody>
					{#each queues as q (q.jobQueueArn)}
						<tr class="border-b border-border/40 hover:bg-muted/30">
							<td class="px-4 py-2 font-mono text-xs">{q.jobQueueName}</td>
							<td class="px-4 py-2 text-right font-mono text-xs">{q.priority}</td>
							<td class="px-4 py-2"><Badge variant={jobStatusVariant(q.state)}>{q.state}</Badge></td>
							<td class="px-4 py-2"><Badge variant={jobStatusVariant(q.status)}>{q.status}</Badge></td>
							<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">
								{q.computeEnvironmentOrder.map((c) => c.computeEnvironment.split('/').pop()).join(', ') || '—'}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>
