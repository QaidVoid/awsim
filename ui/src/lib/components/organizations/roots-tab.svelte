<script lang="ts">
	import { onMount } from 'svelte';
	import { listRoots, type Root } from '$lib/api/organizations';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import GitFork from '@lucide/svelte/icons/git-fork';
	import { toast } from 'svelte-sonner';

	let roots = $state<Root[]>([]);
	let loading = $state(true);

	async function reload() {
		loading = true;
		try {
			const r = await listRoots();
			roots = r.roots;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load roots');
		} finally {
			loading = false;
		}
	}

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{roots.length} root{roots.length === 1 ? '' : 's'}
		</div>
		<Button type="button" variant="outline" size="sm" onclick={reload} disabled={loading}>
			<RefreshCw class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</header>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if loading && roots.length === 0}
			<div class="space-y-2 p-4">
				{#each Array(2) as _, i (i)}
					<Skeleton class="h-7 w-full" />
				{/each}
			</div>
		{:else if roots.length === 0}
			<div class="p-6">
				<EmptyState
					icon={GitFork}
					title="No roots"
					description="Create an organization to populate its root."
				/>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">ID</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Name</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Policy types</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">ARN</th>
					</tr>
				</thead>
				<tbody>
					{#each roots as r (r.id)}
						<tr class="border-b border-border/40 hover:bg-muted/30">
							<td class="px-4 py-2 font-mono text-xs">{r.id}</td>
							<td class="px-4 py-2 font-mono text-xs">{r.name}</td>
							<td class="px-4 py-2">
								<div class="flex flex-wrap gap-1">
									{#each r.policyTypes as p (p.type)}
										<Badge
											variant={p.status === 'ENABLED' ? 'default' : 'outline'}
											class="font-mono text-[10px]"
										>
											{p.type}
										</Badge>
									{:else}
										<span class="text-[11px] text-muted-foreground">—</span>
									{/each}
								</div>
							</td>
							<td class="max-w-md truncate px-4 py-2 font-mono text-[11px] text-muted-foreground">
								{r.arn}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>
