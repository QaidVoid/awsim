<script lang="ts">
	import {
		createCluster,
		deleteCluster,
		type Cluster
	} from '$lib/api/ecs';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter
	} from '$lib/components/ui/dialog';
	import { EmptyState } from '$lib/components/service';
	import { toast } from 'svelte-sonner';
	import Plus from '@lucide/svelte/icons/plus';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Boxes from '@lucide/svelte/icons/boxes';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		clusters: Cluster[];
		loading: boolean;
		selectedArn: string | null;
		onReload: () => void;
		onSelect: (cluster: Cluster) => void;
	}

	let { clusters, loading, selectedArn, onReload, onSelect }: Props = $props();

	let createOpen = $state(false);
	let newName = $state('');
	let creating = $state(false);

	async function handleCreate(e: Event) {
		e.preventDefault();
		if (!newName.trim()) return;
		creating = true;
		try {
			await createCluster(newName.trim());
			toast.success(`Created cluster ${newName.trim()}`);
			newName = '';
			createOpen = false;
			onReload();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Create failed');
		} finally {
			creating = false;
		}
	}

	async function handleDelete(cluster: Cluster) {
		if (!confirm(`Delete cluster "${cluster.name}"?`)) return;
		try {
			await deleteCluster(cluster.arn);
			toast.success(`Deleted ${cluster.name}`);
			onReload();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		}
	}

	function statusVariant(s: string): 'default' | 'secondary' | 'destructive' {
		if (s === 'ACTIVE') return 'default';
		if (s === 'INACTIVE') return 'destructive';
		return 'secondary';
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header
		class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2"
	>
		<div class="text-xs text-muted-foreground">
			{clusters.length} cluster{clusters.length === 1 ? '' : 's'}
		</div>
		<div class="flex items-center gap-2">
			<Button type="button" variant="outline" size="sm" onclick={onReload} disabled={loading}>
				<RefreshCw />
				Refresh
			</Button>
			<Button type="button" size="sm" onclick={() => (createOpen = true)}>
				<Plus />
				Create cluster
			</Button>
		</div>
	</header>

	<div class="min-h-0 flex-1 overflow-y-auto">
		{#if loading && clusters.length === 0}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if clusters.length === 0}
			<div class="p-6">
				<EmptyState
					icon={Boxes}
					title="No clusters"
					description="Create one to start running tasks and services."
				/>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="border-b border-border bg-background/95 text-left text-muted-foreground">
					<tr>
						<th class="px-4 py-2 font-medium">Name</th>
						<th class="px-4 py-2 font-medium">Status</th>
						<th class="px-4 py-2 text-right font-medium">Services</th>
						<th class="px-4 py-2 text-right font-medium">Running</th>
						<th class="px-4 py-2 text-right font-medium">Pending</th>
						<th class="px-4 py-2 font-medium">ARN</th>
						<th class="px-4 py-2 text-right font-medium"></th>
					</tr>
				</thead>
				<tbody>
					{#each clusters as cluster (cluster.arn)}
						<tr
							class="cursor-pointer border-b border-border/40 transition-colors {selectedArn ===
							cluster.arn
								? 'bg-muted'
								: 'hover:bg-muted/40'}"
							onclick={() => onSelect(cluster)}
						>
							<td class="px-4 py-2 font-mono text-xs">{cluster.name}</td>
							<td class="px-4 py-2">
								<Badge variant={statusVariant(cluster.status)}>{cluster.status}</Badge>
							</td>
							<td class="px-4 py-2 text-right">{cluster.activeServices}</td>
							<td class="px-4 py-2 text-right">{cluster.runningTasks}</td>
							<td class="px-4 py-2 text-right">{cluster.pendingTasks}</td>
							<td class="truncate px-4 py-2 font-mono text-[11px] text-muted-foreground">
								{cluster.arn}
							</td>
							<td class="px-4 py-2 text-right">
								<Button
									type="button"
									variant="ghost"
									size="icon-xs"
									onclick={(e) => {
										e.stopPropagation();
										handleDelete(cluster);
									}}
									aria-label="Delete cluster"
								>
									<Trash2 />
								</Button>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>

<Dialog open={createOpen} onOpenChange={(o) => (createOpen = o)}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Create cluster</DialogTitle>
			<DialogDescription>Provision a new ECS cluster.</DialogDescription>
		</DialogHeader>
		<form onsubmit={handleCreate} class="flex flex-col gap-4 py-2">
			<div class="flex flex-col gap-1.5">
				<Label for="ecs-cluster-name">Cluster name</Label>
				<Input
					id="ecs-cluster-name"
					bind:value={newName}
					placeholder="my-cluster"
					required
				/>
			</div>
			<DialogFooter>
				<Button type="button" variant="ghost" onclick={() => (createOpen = false)}>
					Cancel
				</Button>
				<Button type="submit" disabled={creating || !newName.trim()}>
					<Plus />
					{creating ? 'Creating...' : 'Create'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
