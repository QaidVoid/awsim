<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listRoots,
		listOrganizationalUnitsForParent,
		type OrganizationalUnit,
		type Root
	} from '$lib/api/organizations';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Network from '@lucide/svelte/icons/network';
	import Plus from '@lucide/svelte/icons/plus';
	import { toast } from 'svelte-sonner';
	import CreateOuDialog from './create-ou-dialog.svelte';

	let ous = $state<OrganizationalUnit[]>([]);
	let roots = $state<Root[]>([]);
	let loading = $state(true);
	let createOpen = $state(false);

	async function reload() {
		loading = true;
		try {
			const r = await listRoots();
			roots = r.roots;
			const collected: OrganizationalUnit[] = [];
			for (const root of roots) {
				try {
					const o = await listOrganizationalUnitsForParent(root.id);
					collected.push(...o.ous);
				} catch {
					// Skip roots whose listing fails (e.g. permissions).
				}
			}
			ous = collected;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load OUs');
		} finally {
			loading = false;
		}
	}

	function rootName(id?: string): string {
		const r = roots.find((x) => x.id === id);
		return r ? r.name : id ?? '';
	}

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{ous.length} OU{ous.length === 1 ? '' : 's'}
		</div>
		<div class="flex items-center gap-2">
			<Button type="button" variant="outline" size="sm" onclick={reload} disabled={loading}>
				<RefreshCw class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button type="button" size="sm" onclick={() => (createOpen = true)}>
				<Plus />
				Create OU
			</Button>
		</div>
	</header>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if loading && ous.length === 0}
			<div class="space-y-2 p-4">
				{#each Array(4) as _, i (i)}
					<Skeleton class="h-7 w-full" />
				{/each}
			</div>
		{:else if ous.length === 0}
			<div class="p-6">
				<EmptyState
					icon={Network}
					title="No organizational units"
					description="Create an OU under a root to start grouping accounts."
				>
					{#snippet action()}
						<Button onclick={() => (createOpen = true)}>
							<Plus />
							Create your first OU
						</Button>
					{/snippet}
				</EmptyState>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">ID</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Name</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Parent root</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">ARN</th>
					</tr>
				</thead>
				<tbody>
					{#each ous as o (o.id)}
						<tr class="border-b border-border/40 hover:bg-muted/30">
							<td class="px-4 py-2 font-mono text-xs">{o.id}</td>
							<td class="px-4 py-2 font-mono text-xs">{o.name}</td>
							<td class="px-4 py-2">
								<Badge variant="outline" class="font-mono text-[10px]">{rootName(o.parentId)}</Badge>
							</td>
							<td class="max-w-md truncate px-4 py-2 font-mono text-[11px] text-muted-foreground">
								{o.arn}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>

<CreateOuDialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={reload}
/>
