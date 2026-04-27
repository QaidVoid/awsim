<script lang="ts">
	import { EmptyState } from '$lib/components/service';
	import LogOut from '@lucide/svelte/icons/log-out';

	interface Props {
		outputs: { key: string; value: string; description?: string }[];
	}

	let { outputs }: Props = $props();
</script>

<div class="min-h-0 flex-1 overflow-auto">
	{#if outputs.length === 0}
		<div class="p-6">
			<EmptyState
				icon={LogOut}
				title="No outputs"
				description="This stack exposes no outputs."
			/>
		</div>
	{:else}
		<table class="w-full text-sm">
			<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
				<tr>
					<th class="px-4 py-2 text-left font-medium text-muted-foreground">Key</th>
					<th class="px-4 py-2 text-left font-medium text-muted-foreground">Value</th>
					<th class="px-4 py-2 text-left font-medium text-muted-foreground">Description</th>
				</tr>
			</thead>
			<tbody>
				{#each outputs as o (o.key)}
					<tr class="border-b border-border/40 hover:bg-muted/30">
						<td class="px-4 py-2 font-mono text-xs">{o.key}</td>
						<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">{o.value}</td>
						<td class="px-4 py-2 text-xs text-muted-foreground">{o.description ?? ''}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	{/if}
</div>
