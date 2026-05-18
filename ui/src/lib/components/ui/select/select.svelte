<script lang="ts">
	import { Select as SelectPrimitive } from "bits-ui";

	// The whole app uses single-select. Constraining to the single
	// variant keeps `value` a plain `string` so `bind:value` on this
	// wrapper type-checks (the raw RootProps union widens it to
	// string | string[] and breaks the bind).
	type SingleProps = SelectPrimitive.RootProps & { type: "single" };

	let {
		value = $bindable(),
		open = $bindable(false),
		...restProps
	}: SingleProps = $props();
</script>

<SelectPrimitive.Root bind:value bind:open {...restProps} />
