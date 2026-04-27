<script lang="ts">
	import { onMount } from 'svelte';
	import { listPolicies, type Policy } from '$lib/api/organizations';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import { toast } from 'svelte-sonner';

	interface Props {
		onSelect: (policy: Policy) => void;
	}

	let { onSelect }: Props = $props();

	let policies = $state<Policy[]>([]);
	let loading = $state(true);

	async function reload() {
		loading = true;
		try {
			const r = await listPolicies('SERVICE_CONTROL_POLICY');
			policies = r.policies;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load policies');
		} finally {
			loading = false;
		}
	}

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{policies.length} SCP{policies.length === 1 ? '' : 's'}
		</div>
		<Button type="button" variant="outline" size="sm" onclick={reload} disabled={loading}>
			<RefreshCw class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</header>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if loading && policies.length === 0}
			<div class="space-y-2 p-4">
				{#each Array(4) as _, i (i)}
					<Skeleton class="h-7 w-full" />
				{/each}
			</div>
		{:else if policies.length === 0}
			<div class="p-6">
				<EmptyState
					icon={ShieldCheck}
					title="No service control policies"
					description="No SCPs defined for this organization."
				/>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">ID</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Name</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Type</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Managed</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Description</th>
					</tr>
				</thead>
				<tbody>
					{#each policies as p (p.id)}
						<tr
							class="cursor-pointer border-b border-border/40 hover:bg-muted/30"
							onclick={() => onSelect(p)}
						>
							<td class="px-4 py-2 font-mono text-xs">{p.id}</td>
							<td class="px-4 py-2 font-mono text-xs">{p.name}</td>
							<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">{p.type}</td>
							<td class="px-4 py-2">
								<Badge variant={p.awsManaged ? 'secondary' : 'outline'}>
									{p.awsManaged ? 'AWS' : 'Customer'}
								</Badge>
							</td>
							<td class="px-4 py-2 text-xs text-muted-foreground">{p.description ?? ''}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>
