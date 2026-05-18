<script lang="ts">
	/**
	 * DetailPage - the canonical route-based detail shell.
	 *
	 * Standardizes the chrome the IAM users/groups/roles/policies detail
	 * routes hand-roll: a back affordance + title + subtitle/ARN header,
	 * an optional left section nav, and a main content pane. Markup and
	 * classes are reproduced verbatim from those routes so adopting this
	 * is a zero-regression refactor. Per-section buttons use DetailNavItem.
	 */
	import type { Snippet } from 'svelte';
	import { goto } from '$app/navigation';
	import { route } from '$lib/url';
	import { cn } from '$lib/utils';
	import { Button } from '$lib/components/ui/button';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';

	interface Props {
		/** Title shown in the header (resource name). */
		title: string;
		/** Secondary line under the title (ARN, id, ...). */
		subtitle?: string | null;
		/** Path the back button navigates to (passed through route()). */
		backHref: string;
		/** Back button tooltip / aria. */
		backLabel?: string;
		/** Whether the resource is still loading. */
		loading?: boolean;
		/** Optional left section nav (typically DetailNavItem buttons). */
		nav?: Snippet;
		/** Optional header actions on the right of the title row. */
		headerActions?: Snippet;
		/** Main content pane. */
		children: Snippet;
		class?: string;
	}

	let {
		title,
		subtitle = null,
		backHref,
		backLabel = 'Back',
		loading = false,
		nav,
		headerActions,
		children,
		class: className
	}: Props = $props();
</script>

<div class={cn('flex h-full min-h-0 flex-col overflow-hidden', className)}>
	<header class="flex items-center gap-3 border-b border-border bg-background px-6 py-3">
		<Button
			variant="ghost"
			size="icon-sm"
			onclick={() => goto(route(backHref))}
			title={backLabel}
		>
			<ArrowLeft class="size-4" />
		</Button>
		<div class="min-w-0 flex-1">
			<h1 class="truncate text-base font-semibold">{title}</h1>
			{#if subtitle}
				<code class="truncate text-xs text-muted-foreground">{subtitle}</code>
			{/if}
		</div>
		{#if headerActions}
			{@render headerActions()}
		{/if}
		{#if loading}
			<span class="text-xs text-muted-foreground">Loading...</span>
		{/if}
	</header>

	<div class="flex flex-1 min-h-0 overflow-hidden">
		{#if nav}
			<nav
				class="flex w-56 shrink-0 flex-col gap-0.5 overflow-y-auto border-r border-border bg-muted/30 p-3"
			>
				{@render nav()}
			</nav>
		{/if}

		<main class="flex min-w-0 flex-1 flex-col overflow-hidden">
			{@render children()}
		</main>
	</div>
</div>
