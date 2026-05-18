<script lang="ts">
	/**
	 * ResourceConsole - the canonical master/detail shell.
	 *
	 * Formalizes the two-pane layout that S3 and DynamoDB hand-rolled:
	 * a ServicePage header, a fixed-width list aside, and a detail pane
	 * that branches between an empty hint, a loading spinner, and the
	 * selected resource (optional header bar + tabbed body). Layout and
	 * classes are reproduced verbatim from those pages so adopting this
	 * is a zero-regression refactor.
	 */
	import type { Snippet } from 'svelte';
	import ServicePage from './service-page.svelte';
	import ErrorState from './error-state.svelte';
	import ListSkeleton from './list-skeleton.svelte';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		/** Page title, forwarded to ServicePage. */
		title: string;
		/** Optional page subtitle, forwarded to ServicePage. */
		description?: string | null;
		/** Page-level header actions (count badge, create button, ...). */
		actions?: Snippet;
		/** Left pane content - the service's list/sidebar component. */
		list: Snippet;
		/** Width of the list pane. Any CSS length. */
		listWidth?: string;
		/** Whether a resource is currently selected. */
		hasSelection: boolean;
		/** Whether the selected resource's detail is still loading. */
		loading?: boolean;
		/** Optional custom loading content; overrides the default spinner. */
		loadingContent?: Snippet;
		/** Text shown in the detail pane when nothing is selected. */
		emptyHint?: string;
		/** Optional custom empty state; overrides emptyHint when given. */
		empty?: Snippet;
		/** Optional header bar rendered above the detail body. */
		detailHeader?: Snippet;
		/** Detail body (tabs, etc.), rendered when selected and not loading. */
		children: Snippet;
		/**
		 * Page-level list gating. When the resource list itself errors /
		 * is loading-empty / is empty, the whole two-pane is replaced by
		 * an error, a skeleton, or the listEmpty CTA - matching the
		 * sqs/sns/ecr/apigateway/appsync/route53 hand-rolled pattern.
		 * All default off so existing adopters are unaffected.
		 */
		listError?: string | null;
		onListRetry?: () => void;
		listLoading?: boolean;
		listIsEmpty?: boolean;
		listSkeletonRows?: number;
		listEmpty?: Snippet;
		/** Forwarded to ServicePage's outer wrapper. */
		class?: string;
	}

	let {
		title,
		description = null,
		actions,
		list,
		listWidth = '280px',
		hasSelection,
		loading = false,
		loadingContent,
		emptyHint = 'Select an item.',
		empty,
		detailHeader,
		children,
		listError = null,
		onListRetry,
		listLoading = false,
		listIsEmpty = false,
		listSkeletonRows = 6,
		listEmpty,
		class: className
	}: Props = $props();
</script>

<ServicePage {title} {description} {actions} class={className}>
	{#if listError}
		<div class="px-6 py-4">
			<ErrorState description={listError} onRetry={onListRetry} />
		</div>
	{:else if listLoading && listIsEmpty}
		<div class="px-6 py-6">
			<ListSkeleton rows={listSkeletonRows} />
		</div>
	{:else if listIsEmpty}
		<div class="px-6 py-12">
			{@render listEmpty?.()}
		</div>
	{:else}
	<div
		class="grid h-full min-h-0 divide-x divide-border"
		style="grid-template-columns: {listWidth} minmax(0, 1fr)"
	>
		<aside class="min-h-0 overflow-hidden">
			{@render list()}
		</aside>

		<section class="flex min-h-0 min-w-0 flex-col">
			{#if !hasSelection}
				{#if empty}
					{@render empty()}
				{:else}
					<div
						class="flex h-full items-center justify-center p-6 text-sm text-muted-foreground"
					>
						{emptyHint}
					</div>
				{/if}
			{:else if loading}
				{#if loadingContent}
					{@render loadingContent()}
				{:else}
					<div class="flex h-full items-center justify-center p-6 text-muted-foreground">
						<Loader2 class="size-4 animate-spin" />
					</div>
				{/if}
			{:else}
				{#if detailHeader}{@render detailHeader()}{/if}
				{@render children()}
			{/if}
		</section>
	</div>
	{/if}
</ServicePage>
