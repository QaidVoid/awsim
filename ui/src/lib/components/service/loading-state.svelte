<script lang="ts">
	/**
	 * Standard centered loading indicator. Mirrors the inline spinner
	 * the service consoles used so it drops in without visual change.
	 */
	import { cn } from '$lib/utils';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		label?: string;
		class?: string;
	}

	let { label, class: className }: Props = $props();
</script>

<div
	class={cn(
		'flex h-full items-center justify-center gap-2 p-6 text-muted-foreground',
		className
	)}
>
	<Loader2 class="size-4 animate-spin" />
	{#if label}
		<span class="text-sm">{label}</span>
	{/if}
</div>
