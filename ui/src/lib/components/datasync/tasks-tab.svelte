<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listTasks,
		startTaskExecution,
		dsStatusVariant,
		type Task
	} from '$lib/api/datasync';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Play from '@lucide/svelte/icons/play';
	import GitBranch from '@lucide/svelte/icons/git-branch';
	import { toast } from 'svelte-sonner';

	interface Props {
		onSelect: (task: Task) => void;
	}

	let { onSelect }: Props = $props();

	let tasks = $state<Task[]>([]);
	let loading = $state(true);

	async function reload() {
		loading = true;
		try {
			const r = await listTasks();
			tasks = r.tasks;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load tasks');
		} finally {
			loading = false;
		}
	}

	async function handleStart(t: Task, e: Event) {
		e.stopPropagation();
		try {
			await startTaskExecution(t.taskArn);
			toast.success('Task execution started');
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Start failed');
		}
	}

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{tasks.length} task{tasks.length === 1 ? '' : 's'}
		</div>
		<Button type="button" variant="outline" size="sm" onclick={reload} disabled={loading}>
			<RefreshCw class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</header>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if loading && tasks.length === 0}
			<div class="space-y-2 p-4">
				{#each Array(4) as _, i (i)}
					<Skeleton class="h-7 w-full" />
				{/each}
			</div>
		{:else if tasks.length === 0}
			<div class="p-6">
				<EmptyState icon={GitBranch} title="No tasks" description="No DataSync tasks defined." />
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Name</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Status</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">ARN</th>
						<th class="px-4 py-2 text-right font-medium text-muted-foreground"></th>
					</tr>
				</thead>
				<tbody>
					{#each tasks as t (t.taskArn)}
						<tr
							class="cursor-pointer border-b border-border/40 hover:bg-muted/30"
							onclick={() => onSelect(t)}
						>
							<td class="px-4 py-2 font-mono text-xs">{t.name ?? '—'}</td>
							<td class="px-4 py-2">
								<Badge variant={dsStatusVariant(t.status)}>{t.status}</Badge>
							</td>
							<td class="max-w-md truncate px-4 py-2 font-mono text-[11px] text-muted-foreground">
								{t.taskArn}
							</td>
							<td class="px-4 py-2 text-right">
								<Button
									type="button"
									variant="ghost"
									size="icon-xs"
									onclick={(e) => handleStart(t, e)}
									aria-label="Start execution"
								>
									<Play />
								</Button>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>
