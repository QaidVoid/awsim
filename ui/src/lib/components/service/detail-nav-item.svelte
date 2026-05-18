<script lang="ts">
	/**
	 * A single section button for DetailPage's left nav. Encapsulates the
	 * active/idle styling that the IAM detail routes duplicated inline.
	 */
	import type { Component } from 'svelte';
	import { cn } from '$lib/utils';

	interface Props {
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		icon: Component<any>;
		label: string;
		active: boolean;
		onclick: () => void;
	}

	let { icon: Icon, label, active, onclick }: Props = $props();
</script>

<button
	type="button"
	class={cn(
		'flex items-center gap-2 rounded px-3 py-2 text-left text-sm transition-colors',
		active
			? 'bg-primary/15 font-medium text-primary'
			: 'text-muted-foreground hover:bg-muted hover:text-foreground'
	)}
	{onclick}
>
	<Icon class="size-4 shrink-0" />
	{label}
</button>
