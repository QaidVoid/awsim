<script lang="ts">
	/**
	 * Left pane: list of CloudWatch log groups with select / create / delete.
	 */
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import Plus from '@lucide/svelte/icons/plus';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import ScrollText from '@lucide/svelte/icons/scroll-text';
	import { toast } from 'svelte-sonner';
	import {
		createLogGroup,
		deleteLogGroup,
		type LogGroup,
	} from '$lib/api/cloudwatch-logs';
	import { bytesHuman } from '$lib/format';
	import { EmptyState } from '$lib/components/service';
	import { cn } from '$lib/utils';

	interface Props {
		groups: LogGroup[];
		selected: string | null;
		loading: boolean;
		onSelect: (name: string) => void;
		onRefresh: () => Promise<void> | void;
	}

	let { groups, selected, loading, onSelect, onRefresh }: Props = $props();

	let creating = $state(false);
	let newName = $state('');
	let busy = $state(false);

	async function handleCreate() {
		const name = newName.trim();
		if (!name) return;
		busy = true;
		try {
			await createLogGroup(name);
			toast.success(`Created ${name}`);
			newName = '';
			creating = false;
			await onRefresh();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create log group');
		} finally {
			busy = false;
		}
	}

	async function handleDelete(name: string, ev: MouseEvent) {
		ev.stopPropagation();
		if (!confirm(`Delete log group ${name}?`)) return;
		try {
			await deleteLogGroup(name);
			toast.success(`Deleted ${name}`);
			await onRefresh();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete log group');
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col border-r border-border">
	<header class="flex shrink-0 items-center justify-between gap-2 border-b border-border px-3 py-2">
		<h2 class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
			Log groups · {groups.length}
		</h2>
		<div class="flex items-center gap-1">
			<Button
				size="sm"
				variant="ghost"
				class="h-7 px-2"
				onclick={() => onRefresh()}
				title="Refresh"
				aria-label="Refresh log groups"
			>
				<RefreshCw class={cn('size-3.5', loading && 'animate-spin')} />
			</Button>
			<Button
				size="sm"
				variant="ghost"
				class="h-7 px-2"
				onclick={() => (creating = !creating)}
				title="New log group"
				aria-label="Create log group"
			>
				<Plus class="size-3.5" />
			</Button>
		</div>
	</header>

	{#if creating}
		<div class="flex shrink-0 flex-col gap-2 border-b border-border bg-muted/20 p-2">
			<Input
				bind:value={newName}
				placeholder="/my/log-group"
				class="h-7 text-xs"
				aria-label="New log group name"
				onkeydown={(e: KeyboardEvent) => e.key === 'Enter' && handleCreate()}
			/>
			<div class="flex items-center gap-2">
				<Button size="sm" class="h-7 px-2 text-xs" onclick={handleCreate} disabled={busy}>
					{busy ? 'Creating…' : 'Create'}
				</Button>
				<Button
					size="sm"
					variant="ghost"
					class="h-7 px-2 text-xs"
					onclick={() => {
						creating = false;
						newName = '';
					}}
				>
					Cancel
				</Button>
			</div>
		</div>
	{/if}

	<div class="min-h-0 flex-1 overflow-y-auto">
		{#if groups.length === 0 && !loading}
			<div class="p-4">
				<EmptyState
					icon={ScrollText}
					title="No log groups"
					description="Create one above or via the AWS CLI."
				/>
			</div>
		{:else}
			<ul>
				{#each groups as g (g.name)}
					{@const isSel = selected === g.name}
					<li
						class={cn(
							'group flex items-start gap-2 border-b border-border/40 px-3 py-2 text-xs transition-colors',
							isSel ? 'bg-muted/60' : 'hover:bg-muted/30'
						)}
					>
						<button
							type="button"
							onclick={() => onSelect(g.name)}
							class="flex flex-1 min-w-0 items-start gap-2 text-left"
						>
							<ScrollText
								class={cn(
									'mt-0.5 size-3.5 shrink-0',
									isSel ? 'text-primary' : 'text-muted-foreground'
								)}
							/>
							<div class="min-w-0 flex-1">
								<div class="truncate font-mono text-[12px] text-foreground">{g.name}</div>
								<div class="mt-0.5 text-[10px] text-muted-foreground">
									{bytesHuman(g.storedBytes)}
									{#if g.retentionDays != null}
										· {g.retentionDays}d retention
									{:else}
										· never expire
									{/if}
								</div>
							</div>
						</button>
						<button
							type="button"
							onclick={(e) => handleDelete(g.name, e)}
							class="opacity-0 group-hover:opacity-100 hover:text-rose-400"
							title="Delete"
							aria-label={`Delete log group ${g.name}`}
						>
							<Trash2 class="size-3.5" />
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>
