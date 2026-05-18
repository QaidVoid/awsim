<script lang="ts">
	import type { Snippet } from 'svelte';
	import { cn } from '$lib/utils';

	interface Props {
		title: string;
		description?: string | null;
		actions?: Snippet;
		toolbar?: Snippet;
		children: Snippet;
		class?: string;
	}

	let {
		title,
		description = null,
		actions,
		toolbar,
		children,
		class: className
	}: Props = $props();
</script>

<div class={cn('flex h-full min-h-0 flex-col overflow-hidden', className)}>
	<header
		class="flex shrink-0 items-start justify-between gap-4 border-b border-border bg-gradient-to-b from-card/50 to-transparent px-6 py-3.5"
	>
		<div class="min-w-0">
			<h1 class="truncate text-lg font-semibold tracking-tight">{title}</h1>
			{#if description}
				<p class="mt-0.5 max-w-3xl text-pretty text-[13px] text-muted-foreground">
					{description}
				</p>
			{/if}
		</div>
		{#if actions}
			<div class="flex shrink-0 items-center gap-2">{@render actions()}</div>
		{/if}
	</header>

	{#if toolbar}
		<div class="flex shrink-0 items-center gap-2 border-b border-border px-6 py-2">
			{@render toolbar()}
		</div>
	{/if}

	<main class="min-h-0 flex-1 overflow-y-auto">
		{@render children()}
	</main>
</div>
