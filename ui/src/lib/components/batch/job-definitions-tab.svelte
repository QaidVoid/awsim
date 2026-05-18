<script lang="ts">
	import { onMount } from 'svelte';
	import { describeJobDefinitions, jobStatusVariant, type JobDefinition } from '$lib/api/batch';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import FileText from '@lucide/svelte/icons/file-text';
	import { toast } from 'svelte-sonner';

	let defs = $state<JobDefinition[]>([]);
	let loading = $state(true);

	async function reload() {
		loading = true;
		try {
			const r = await describeJobDefinitions();
			defs = r.jobDefinitions;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load job definitions');
		} finally {
			loading = false;
		}
	}

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{defs.length} definition{defs.length === 1 ? '' : 's'}
		</div>
		<Button type="button" variant="outline" size="sm" onclick={reload} disabled={loading}>
			<RefreshCw class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</header>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if loading && defs.length === 0}
			<div class="space-y-2 p-4">
				{#each Array(4) as _, i (i)}
					<Skeleton class="h-7 w-full" />
				{/each}
			</div>
		{:else if defs.length === 0}
			<div class="p-6">
				<EmptyState
					icon={FileText}
					title="No job definitions"
					description="Job definitions are reusable templates that describe how a Batch job runs. None have been registered yet."
				/>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Name</th>
						<th class="px-4 py-2 text-right font-medium text-muted-foreground">Revision</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Type</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Status</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Image</th>
					</tr>
				</thead>
				<tbody>
					{#each defs as d (d.jobDefinitionArn)}
						<tr class="border-b border-border/40 hover:bg-muted/30">
							<td class="px-4 py-2 font-mono text-xs">{d.jobDefinitionName}</td>
							<td class="px-4 py-2 text-right font-mono text-xs">{d.revision}</td>
							<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">{d.type}</td>
							<td class="px-4 py-2">
								<Badge variant={jobStatusVariant(d.status)}>{d.status}</Badge>
							</td>
							<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">
								{d.containerImage ?? '—'}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>
