<script lang="ts">
	import { onMount } from 'svelte';
	import {
		describeComputeEnvironments,
		jobStatusVariant,
		type ComputeEnvironment
	} from '$lib/api/batch';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Cpu from '@lucide/svelte/icons/cpu';
	import { toast } from 'svelte-sonner';

	let envs = $state<ComputeEnvironment[]>([]);
	let loading = $state(true);

	async function reload() {
		loading = true;
		try {
			const r = await describeComputeEnvironments();
			envs = r.computeEnvironments;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load compute environments');
		} finally {
			loading = false;
		}
	}

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{envs.length} environment{envs.length === 1 ? '' : 's'}
		</div>
		<Button type="button" variant="outline" size="sm" onclick={reload} disabled={loading}>
			<RefreshCw class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</header>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if loading && envs.length === 0}
			<div class="space-y-2 p-4">
				{#each Array(4) as _, i (i)}
					<Skeleton class="h-7 w-full" />
				{/each}
			</div>
		{:else if envs.length === 0}
			<div class="p-6">
				<EmptyState
					icon={Cpu}
					title="No compute environments"
					description="Compute environments supply the capacity that Batch jobs run on. None have been provisioned yet."
				/>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Name</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Type</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">State</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Status</th>
					</tr>
				</thead>
				<tbody>
					{#each envs as e (e.computeEnvironmentArn)}
						<tr class="border-b border-border/40 hover:bg-muted/30">
							<td class="px-4 py-2 font-mono text-xs">{e.computeEnvironmentName}</td>
							<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">{e.type}</td>
							<td class="px-4 py-2"><Badge variant={jobStatusVariant(e.state)}>{e.state}</Badge></td>
							<td class="px-4 py-2"><Badge variant={jobStatusVariant(e.status)}>{e.status}</Badge></td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>
