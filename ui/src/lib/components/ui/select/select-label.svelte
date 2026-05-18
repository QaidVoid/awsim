<script lang="ts">
	import { cn } from "$lib/utils.js";
	import { Select as SelectPrimitive } from "bits-ui";

	let {
		ref = $bindable(null),
		class: className,
		...restProps
	}: SelectPrimitive.GroupHeadingProps = $props();
</script>

<SelectPrimitive.GroupHeading
	bind:ref
	data-slot="select-label"
	class={cn("text-muted-foreground px-2 py-1.5 text-xs", className)}
	{...restProps}
/>
