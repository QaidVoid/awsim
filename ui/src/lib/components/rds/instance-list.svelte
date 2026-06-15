<script lang="ts">
	import type { DBInstance } from '$lib/api/rds';
	import { statusVariant } from '$lib/api/rds';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { EmptyState } from '$lib/components/service';
	import InstanceDetail from '$lib/components/rds/instance-detail.svelte';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Database from '@lucide/svelte/icons/database';
	import Search from '@lucide/svelte/icons/search';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';

	interface Props {
		instances: DBInstance[];
		loading: boolean;
		onRefresh: () => void;
		onDeleteInstance: (instance: DBInstance) => void;
	}

	let { instances, loading, onRefresh, onDeleteInstance }: Props = $props();

	let filter = $state('');
	let expandedId = $state<string | null>(null);

	let visible = $derived(
		filter.trim().length === 0
			? instances
			: instances.filter((i) =>
					i.identifier.toLowerCase().includes(filter.trim().toLowerCase())
				)
	);

	function toggle(id: string) {
		expandedId = expandedId === id ? null : id;
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div
		class="flex shrink-0 items-center gap-2 border-b border-border bg-background/40 px-4 py-2"
	>
		<div class="relative flex-1">
			<Search
				class="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground"
			/>
			<input
				type="text"
				bind:value={filter}
				placeholder="Filter instances..."
				class="h-8 w-full rounded-md border border-border bg-background pr-2 pl-7 text-xs outline-none focus:border-ring focus:ring-1 focus:ring-ring"
			/>
		</div>
		<Button variant="ghost" size="icon-sm" onclick={onRefresh} aria-label="Refresh">
			{#if loading}
				<Loader2 class="size-3.5 animate-spin" />
			{:else}
				<RefreshCw class="size-3.5" />
			{/if}
		</Button>
	</div>

	<div class="min-h-0 flex-1 overflow-hidden">
		{#if loading && instances.length === 0}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if visible.length === 0}
			<div class="flex h-full items-center justify-center p-6">
				<EmptyState
					icon={Database}
					title={filter ? 'No matches' : 'No instances yet'}
					description={filter
						? 'Try a different filter.'
						: 'Create your first DB instance.'}
				/>
			</div>
		{:else}
			<div class="h-full overflow-auto">
				<table class="w-full text-xs">
					<thead
						class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm"
					>
						<tr>
							<th class="w-6"></th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">
								Identifier
							</th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">Engine</th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">Status</th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">Class</th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">Endpoint</th>
						</tr>
					</thead>
					<tbody>
						{#each visible as inst (inst.identifier)}
							<tr
								class="cursor-pointer border-b border-border/40 transition-colors hover:bg-muted/40 {expandedId ===
								inst.identifier
									? 'bg-muted/60'
									: ''}"
								onclick={() => toggle(inst.identifier)}
							>
								<td class="pl-2 text-muted-foreground">
									<ChevronRight
										class="size-3.5 transition-transform {expandedId === inst.identifier
											? 'rotate-90'
											: ''}"
									/>
								</td>
								<td class="px-3 py-2 font-mono">{inst.identifier}</td>
								<td class="px-3 py-2 font-mono">
									{inst.engine}{inst.engineVersion ? ` ${inst.engineVersion}` : ''}
								</td>
								<td class="px-3 py-2">
									<Badge variant={statusVariant(inst.status)}>
										{inst.status || 'unknown'}
									</Badge>
								</td>
								<td class="px-3 py-2 font-mono text-muted-foreground">
									{inst.instanceClass}
								</td>
								<td class="px-3 py-2 font-mono text-muted-foreground">
									{inst.endpoint || '—'}{inst.port ? `:${inst.port}` : ''}
								</td>
							</tr>
							{#if expandedId === inst.identifier}
								<tr>
									<td colspan="6" class="p-0">
										<InstanceDetail instance={inst} {onDeleteInstance} />
									</td>
								</tr>
							{/if}
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>
</div>
