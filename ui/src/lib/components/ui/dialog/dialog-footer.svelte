<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";
	import { Dialog as DialogPrimitive } from "bits-ui";
	import { Button } from "$lib/components/ui/button/index.js";

	let {
		ref = $bindable(null),
		class: className,
		children,
		showCloseButton = false,
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLDivElement>> & {
		showCloseButton?: boolean;
	} = $props();
</script>

<div
	bind:this={ref}
	data-slot="dialog-footer"
	class={cn(
		"sticky bottom-0 z-10 -mx-6 mt-2 flex flex-col-reverse gap-2 border-t border-border/60 bg-popover px-6 pt-4 pb-6 sm:flex-row sm:justify-end",
		className
	)}
	{...restProps}
>
	{@render children?.()}
	{#if showCloseButton}
		<DialogPrimitive.Close>
			{#snippet child({ props })}
				<Button variant="outline" {...props}>Close</Button>
			{/snippet}
		</DialogPrimitive.Close>
	{/if}
</div>
