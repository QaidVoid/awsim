<script lang="ts">
	import {
		createCluster,
		deleteCluster,
		type Cluster,
		type CreateClusterInput
	} from '$lib/api/eks';
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
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import { toast } from 'svelte-sonner';
	import Plus from '@lucide/svelte/icons/plus';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Server from '@lucide/svelte/icons/server';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		clusters: Cluster[];
		loading: boolean;
		selectedName: string | null;
		onReload: () => void;
		onSelect: (cluster: Cluster) => void;
	}

	let { clusters, loading, selectedName, onReload, onSelect }: Props = $props();

	let createOpen = $state(false);
	let creating = $state(false);
	let newCluster = $state<CreateClusterInput>({
		name: '',
		version: '1.31',
		roleArn: 'arn:aws:iam::000000000000:role/eks-cluster'
	});
	let deleteTarget = $state<Cluster | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	function statusVariant(s: string): 'default' | 'secondary' | 'destructive' | 'outline' {
		if (s === 'ACTIVE') return 'default';
		if (s === 'FAILED') return 'destructive';
		if (s === 'CREATING' || s === 'UPDATING') return 'secondary';
		return 'outline';
	}

	async function submit(e: Event) {
		e.preventDefault();
		if (!newCluster.name.trim()) return;
		creating = true;
		try {
			await createCluster({
				name: newCluster.name.trim(),
				version: newCluster.version.trim(),
				roleArn: newCluster.roleArn.trim()
			});
			toast.success(`Created cluster ${newCluster.name.trim()}`);
			createOpen = false;
			newCluster = {
				name: '',
				version: '1.31',
				roleArn: 'arn:aws:iam::000000000000:role/eks-cluster'
			};
			onReload();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Create failed');
		} finally {
			creating = false;
		}
	}

	function handleDelete(c: Cluster) {
		deleteTarget = c;
		deleteOpen = true;
	}

	async function confirmDelete() {
		const c = deleteTarget;
		if (!c) return;
		deleteBusy = true;
		try {
			await deleteCluster(c.name);
			toast.success(`Deleted ${c.name}`);
			deleteOpen = false;
			deleteTarget = null;
			onReload();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2">
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
					icon={Server}
					title="No clusters"
					description="Create one to manage Kubernetes nodes & Fargate profiles."
				/>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="border-b border-border bg-background/95 text-left text-muted-foreground">
					<tr>
						<th class="px-4 py-2 font-medium">Name</th>
						<th class="px-4 py-2 font-medium">Version</th>
						<th class="px-4 py-2 font-medium">Status</th>
						<th class="px-4 py-2 font-medium">Endpoint</th>
						<th class="px-4 py-2 font-medium">ARN</th>
						<th class="px-4 py-2 text-right font-medium"></th>
					</tr>
				</thead>
				<tbody>
					{#each clusters as c (c.name)}
						<tr
							class="cursor-pointer border-b border-border/40 transition-colors {selectedName === c.name ? 'bg-muted' : 'hover:bg-muted/40'}"
							onclick={() => onSelect(c)}
						>
							<td class="px-4 py-2 font-mono text-xs">{c.name}</td>
							<td class="px-4 py-2 text-xs text-muted-foreground">{c.version || '—'}</td>
							<td class="px-4 py-2"><Badge variant={statusVariant(c.status)}>{c.status}</Badge></td>
							<td class="truncate px-4 py-2 font-mono text-[11px] text-muted-foreground">{c.endpoint || '—'}</td>
							<td class="truncate px-4 py-2 font-mono text-[11px] text-muted-foreground">{c.arn}</td>
							<td class="px-4 py-2 text-right">
								<Button
									type="button"
									variant="ghost"
									size="icon-xs"
									aria-label="Delete cluster"
									onclick={(e) => {
										e.stopPropagation();
										handleDelete(c);
									}}
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
			<DialogTitle>Create EKS cluster</DialogTitle>
			<DialogDescription>Provision an EKS control plane.</DialogDescription>
		</DialogHeader>
		<form onsubmit={submit} class="flex flex-col gap-3 py-2">
			<div class="flex flex-col gap-1.5">
				<Label for="eks-name">Cluster name</Label>
				<Input id="eks-name" bind:value={newCluster.name} placeholder="my-cluster" required />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="eks-version">Kubernetes version</Label>
				<Input id="eks-version" bind:value={newCluster.version} placeholder="1.31" />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="eks-role">Role ARN</Label>
				<Input id="eks-role" bind:value={newCluster.roleArn} class="font-mono text-xs" />
			</div>
			<DialogFooter>
				<Button type="button" variant="ghost" onclick={() => (createOpen = false)}>Cancel</Button>
				<Button type="submit" disabled={creating || !newCluster.name.trim()}>
					<Plus />
					{creating ? 'Creating...' : 'Create'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete cluster?"
	description={`Delete cluster ${deleteTarget?.name ?? ''}.`}
	busy={deleteBusy}
	onConfirm={confirmDelete}
	onClose={() => (deleteOpen = false)}
/>
