<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listTaskExecutions,
		dsStatusVariant,
		type TaskExecution
	} from '$lib/api/datasync';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Activity from '@lucide/svelte/icons/activity';
	import { toast } from 'svelte-sonner';

	let executions = $state<TaskExecution[]>([]);
	let loading = $state(true);

	async function reload() {
		loading = true;
		try {
			const r = await listTaskExecutions();
			executions = r.executions;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load executions');
		} finally {
			loading = false;
		}
	}

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{executions.length} execution{executions.length === 1 ? '' : 's'}
		</div>
		<Button type="button" variant="outline" size="sm" onclick={reload} disabled={loading}>
			<RefreshCw class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</header>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if loading && executions.length === 0}
			<div class="space-y-2 p-4">
				{#each Array(4) as _, i (i)}
					<Skeleton class="h-7 w-full" />
				{/each}
			</div>
		{:else if executions.length === 0}
			<div class="p-6">
				<EmptyState
					icon={Activity}
					title="No executions"
					description="Start a task to see its execution here."
				/>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Execution ARN</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Status</th>
					</tr>
				</thead>
				<tbody>
					{#each executions as e (e.taskExecutionArn)}
						<tr class="border-b border-border/40 hover:bg-muted/30">
							<td class="max-w-md truncate px-4 py-2 font-mono text-[11px] text-muted-foreground">
								{e.taskExecutionArn}
							</td>
							<td class="px-4 py-2">
								<Badge variant={dsStatusVariant(e.status)}>{e.status}</Badge>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>
