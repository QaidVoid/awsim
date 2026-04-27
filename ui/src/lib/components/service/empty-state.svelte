<script lang="ts">
	import type { Component, Snippet } from 'svelte';
	import { cn } from '$lib/utils';

	interface Props {
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		icon?: Component<any>;
		title: string;
		description?: string | null;
		action?: Snippet;
		class?: string;
	}

	let { icon: Icon, title, description = null, action, class: className }: Props = $props();
</script>

<div
	class={cn(
		'flex flex-col items-center justify-center gap-3 rounded-md border border-dashed border-border/60 px-6 py-16 text-center',
		className
	)}
>
	{#if Icon}
		<Icon class="size-8 text-muted-foreground" />
	{/if}
	<div>
		<p class="text-sm font-medium">{title}</p>
		{#if description}
			<p class="mt-1 text-xs text-muted-foreground">{description}</p>
		{/if}
	</div>
	{#if action}
		{@render action()}
	{/if}
</div>
