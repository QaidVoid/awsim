<script lang="ts">
	/**
	 * Standard error surface with an optional retry. Visual sibling of
	 * EmptyState (same dashed card) but destructive-toned, so a failed
	 * fetch reads clearly instead of looking like "no data".
	 */
	import { cn } from '$lib/utils';
	import { Button } from '$lib/components/ui/button';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';

	interface Props {
		title?: string;
		description?: string | null;
		onRetry?: () => void;
		retryLabel?: string;
		class?: string;
	}

	let {
		title = 'Something went wrong',
		description = null,
		onRetry,
		retryLabel = 'Retry',
		class: className
	}: Props = $props();
</script>

<div
	class={cn(
		'flex flex-col items-center justify-center gap-3 rounded-md border border-dashed border-destructive/40 px-6 py-16 text-center',
		className
	)}
>
	<TriangleAlert class="size-8 text-destructive" />
	<div>
		<p class="text-sm font-medium">{title}</p>
		{#if description}
			<p class="mt-1 text-xs text-muted-foreground">{description}</p>
		{/if}
	</div>
	{#if onRetry}
		<Button variant="outline" size="sm" onclick={onRetry}>
			<RefreshCw class="size-3.5" />
			{retryLabel}
		</Button>
	{/if}
</div>
