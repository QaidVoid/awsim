<script lang="ts">
	import { listTasks, type Cluster, type Task } from '$lib/api/ecs';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import { toast } from 'svelte-sonner';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Container from '@lucide/svelte/icons/container';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		cluster: Cluster | null;
		onSelect: (task: Task) => void;
	}

	let { cluster, onSelect }: Props = $props();

	let tasks = $state<Task[]>([]);
	let loading = $state(false);
	let lastArn = $state('');

	$effect(() => {
		if (cluster && cluster.arn !== lastArn) {
			lastArn = cluster.arn;
			void load();
		}
		if (!cluster) {
			tasks = [];
			lastArn = '';
		}
	});

	async function load() {
		if (!cluster) return;
		loading = true;
		try {
			const r = await listTasks(cluster.arn);
			tasks = r.tasks;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load tasks');
		} finally {
			loading = false;
		}
	}

	function statusVariant(s: string): 'default' | 'secondary' | 'destructive' | 'outline' {
		if (s === 'RUNNING') return 'default';
		if (s === 'STOPPED') return 'destructive';
		if (s === 'PENDING' || s === 'PROVISIONING' || s === 'ACTIVATING') return 'secondary';
		return 'outline';
	}

	function shortArn(arn: string): string {
		return arn.split('/').pop() ?? arn;
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{cluster ? `Tasks in ${cluster.name}` : 'Pick a cluster from the Clusters tab'}
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
					icon={Container}
					title="No cluster selected"
					description="Choose a cluster on the Clusters tab to inspect tasks."
				/>
			</div>
		{:else if loading && tasks.length === 0}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if tasks.length === 0}
			<div class="p-6">
				<EmptyState icon={Container} title="No tasks" description="This cluster has no running or stopped tasks." />
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="border-b border-border bg-background/95 text-left text-muted-foreground">
					<tr>
						<th class="px-4 py-2 font-medium">Task ID</th>
						<th class="px-4 py-2 font-medium">Last status</th>
						<th class="px-4 py-2 font-medium">Desired</th>
						<th class="px-4 py-2 font-medium">Task def</th>
						<th class="px-4 py-2 font-medium">Launch</th>
						<th class="px-4 py-2 text-right font-medium">CPU</th>
						<th class="px-4 py-2 text-right font-medium">Mem</th>
					</tr>
				</thead>
				<tbody>
					{#each tasks as t (t.arn)}
						<tr
							class="cursor-pointer border-b border-border/40 hover:bg-muted/40"
							onclick={() => onSelect(t)}
						>
							<td class="px-4 py-2 font-mono text-[11px]">{shortArn(t.arn)}</td>
							<td class="px-4 py-2">
								<Badge variant={statusVariant(t.lastStatus)}>{t.lastStatus}</Badge>
							</td>
							<td class="px-4 py-2 text-xs text-muted-foreground">{t.desiredStatus}</td>
							<td class="px-4 py-2 font-mono text-[11px]">{shortArn(t.taskDefinitionArn)}</td>
							<td class="px-4 py-2 text-xs text-muted-foreground">{t.launchType || '—'}</td>
							<td class="px-4 py-2 text-right">{t.cpu || '—'}</td>
							<td class="px-4 py-2 text-right">{t.memory || '—'}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>
