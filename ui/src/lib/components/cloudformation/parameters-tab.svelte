<script lang="ts">
	import { EmptyState } from '$lib/components/service';
	import SlidersHorizontal from '@lucide/svelte/icons/sliders-horizontal';

	interface Props {
		parameters: { key: string; value: string }[];
	}

	let { parameters }: Props = $props();
</script>

<div class="min-h-0 flex-1 overflow-auto">
	{#if parameters.length === 0}
		<div class="p-6">
			<EmptyState
				icon={SlidersHorizontal}
				title="No parameters"
				description="This stack defines no template parameters."
			/>
		</div>
	{:else}
		<table class="w-full text-sm">
			<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
				<tr>
					<th class="px-4 py-2 text-left font-medium text-muted-foreground">Key</th>
					<th class="px-4 py-2 text-left font-medium text-muted-foreground">Value</th>
				</tr>
			</thead>
			<tbody>
				{#each parameters as p (p.key)}
					<tr class="border-b border-border/40 hover:bg-muted/30">
						<td class="px-4 py-2 font-mono text-xs">{p.key}</td>
						<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">{p.value}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	{/if}
</div>
