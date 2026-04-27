<script lang="ts">
	/**
	 * Middle pane: log streams scoped to a selected log group.
	 */
	import { Button } from '$lib/components/ui/button';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import GitBranch from '@lucide/svelte/icons/git-branch';
	import { type LogStream } from '$lib/api/cloudwatch-logs';
	import { bytesHuman, relativeTime } from '$lib/format';
	import { EmptyState } from '$lib/components/service';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { cn } from '$lib/utils';

	interface Props {
		group: string | null;
		streams: LogStream[];
		selected: string | null;
		loading: boolean;
		onSelect: (name: string) => void;
		onRefresh: () => Promise<void> | void;
	}

	let { group, streams, selected, loading, onSelect, onRefresh }: Props = $props();
</script>

<div class="flex h-full min-h-0 flex-col border-r border-border">
	<header class="flex shrink-0 items-center justify-between gap-2 border-b border-border px-3 py-2">
		<h2 class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
			Streams · {streams.length}
		</h2>
		<Button
			size="sm"
			variant="ghost"
			class="h-7 px-2"
			onclick={() => onRefresh()}
			title="Refresh"
			aria-label="Refresh streams"
			disabled={!group}
		>
			<RefreshCw class={cn('size-3.5', loading && 'animate-spin')} />
		</Button>
	</header>

	<div class="min-h-0 flex-1 overflow-y-auto">
		{#if !group}
			<div class="p-4">
				<EmptyState
					icon={GitBranch}
					title="No log group selected"
					description="Pick a log group on the left to see its streams."
				/>
			</div>
		{:else if loading && streams.length === 0}
			<div class="space-y-2 p-3">
				{#each Array(4) as _, i (i)}
					<Skeleton class="h-10 w-full" />
				{/each}
			</div>
		{:else if streams.length === 0 && !loading}
			<div class="p-4">
				<EmptyState
					icon={GitBranch}
					title="No streams"
					description="This log group has no streams yet."
				/>
			</div>
		{:else}
			<ul>
				{#each streams as s (s.name)}
					{@const isSel = selected === s.name}
					<li>
						<button
							type="button"
							onclick={() => onSelect(s.name)}
							class={cn(
								'flex w-full items-start gap-2 border-b border-border/40 px-3 py-2 text-left text-xs transition-colors',
								isSel ? 'bg-muted/60' : 'hover:bg-muted/30'
							)}
						>
							<GitBranch
								class={cn(
									'mt-0.5 size-3.5 shrink-0',
									isSel ? 'text-primary' : 'text-muted-foreground'
								)}
							/>
							<div class="min-w-0 flex-1">
								<div class="truncate font-mono text-[12px] text-foreground">{s.name}</div>
								<div class="mt-0.5 text-[10px] text-muted-foreground">
									{bytesHuman(s.storedBytes)}
									{#if s.lastEventTimestamp}
										· {relativeTime(s.lastEventTimestamp / 1000)}
									{/if}
								</div>
							</div>
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>
