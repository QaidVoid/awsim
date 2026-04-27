<script lang="ts">
	import type { StackSummary } from '$lib/api/cloudformation';
	import { stackStatusVariant } from '$lib/api/cloudformation';
	import { Badge } from '$lib/components/ui/badge';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import Layers from '@lucide/svelte/icons/layers';
	import { cn } from '$lib/utils';

	interface Props {
		stacks: StackSummary[];
		loading: boolean;
		selected: string | null;
		onSelect: (stack: StackSummary) => void;
	}

	let { stacks, loading, selected, onSelect }: Props = $props();

	function fmt(iso: string): string {
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	{#if loading && stacks.length === 0}
		<div class="space-y-2 p-3">
			{#each Array(5) as _, i (i)}
				<Skeleton class="h-14 w-full" />
			{/each}
		</div>
	{:else if stacks.length === 0}
		<div class="p-4">
			<EmptyState
				icon={Layers}
				title="No stacks"
				description="Create one to model AWS resources."
			/>
		</div>
	{:else}
		<ul class="flex-1 overflow-y-auto py-1">
			{#each stacks as stack (stack.stackId || stack.stackName)}
				<li>
					<button
						type="button"
						class={cn(
							'flex w-full flex-col items-start gap-1 border-l-2 px-3 py-2 text-left transition-colors',
							selected === stack.stackName
								? 'border-primary bg-muted'
								: 'border-transparent hover:bg-muted/50'
						)}
						onclick={() => onSelect(stack)}
					>
						<span class="truncate font-mono text-xs text-foreground">
							{stack.stackName}
						</span>
						<div class="flex w-full items-center justify-between gap-2">
							<Badge variant={stackStatusVariant(stack.stackStatus)} class="text-[10px]">
								{stack.stackStatus}
							</Badge>
							<span class="text-[10px] text-muted-foreground">{fmt(stack.creationTime)}</span>
						</div>
						{#if stack.templateDescription}
							<span class="truncate text-[11px] text-muted-foreground">
								{stack.templateDescription}
							</span>
						{/if}
					</button>
				</li>
			{/each}
		</ul>
	{/if}
</div>
