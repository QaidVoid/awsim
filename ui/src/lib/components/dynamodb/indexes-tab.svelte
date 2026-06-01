<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { deleteGsi, type TableDetail } from '$lib/api/dynamodb';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import { EmptyState } from '$lib/components/service';
	import Layers from '@lucide/svelte/icons/layers';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import CreateGsiDialog from './create-gsi-dialog.svelte';

	interface Props {
		detail: TableDetail;
		onUpdated: () => void | Promise<void>;
	}

	let { detail, onUpdated }: Props = $props();

	let hasIndexes = $derived(
		detail.globalSecondaryIndexes.length > 0 || detail.localSecondaryIndexes.length > 0,
	);

	let addOpen = $state(false);
	let deleteOpen = $state(false);
	let deleteTarget = $state<string | null>(null);
	let deleting = $state(false);

	function askDelete(indexName: string) {
		deleteTarget = indexName;
		deleteOpen = true;
	}

	async function confirmDelete() {
		if (!deleteTarget) return;
		deleting = true;
		try {
			await deleteGsi(detail.name, deleteTarget);
			toast.success(`Deleted GSI ${deleteTarget}`);
			deleteOpen = false;
			deleteTarget = null;
			await onUpdated();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete GSI');
		} finally {
			deleting = false;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col gap-4 overflow-y-auto p-4">
	<div class="flex items-center justify-between">
		<p class="text-xs text-muted-foreground">
			GSIs can be added or removed at any time. LSIs must be defined when the table is created
			and stay frozen afterwards (AWS rule).
		</p>
		<Button size="sm" onclick={() => (addOpen = true)}>
			<Plus class="size-3.5" />
			<span class="ml-1">Add GSI</span>
		</Button>
	</div>

	{#if !hasIndexes}
		<div class="flex flex-1 items-center justify-center">
			<EmptyState
				icon={Layers}
				title="No secondary indexes"
				description="This table only has its primary key. Add a GSI above to start querying on alternate keys."
			/>
		</div>
	{:else}
		{#if detail.globalSecondaryIndexes.length > 0}
			<section>
				<h3 class="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
					Global secondary indexes
				</h3>
				<div class="flex flex-col gap-3">
					{#each detail.globalSecondaryIndexes as idx (idx.indexName)}
						<div class="rounded-md border border-border p-3">
							<div class="mb-2 flex items-center justify-between gap-2">
								<span class="font-mono text-sm">{idx.indexName}</span>
								<div class="flex items-center gap-1.5">
									<Badge variant="outline">{idx.projectionType}</Badge>
									<Badge variant={idx.status === 'ACTIVE' ? 'secondary' : 'outline'}>
										{idx.status || 'UNKNOWN'}
									</Badge>
									<Button
										variant="ghost"
										size="icon"
										onclick={() => askDelete(idx.indexName)}
										aria-label={`Delete ${idx.indexName}`}
									>
										<Trash2 class="size-4 text-rose-600" />
									</Button>
								</div>
							</div>
							<dl class="grid grid-cols-[100px_1fr] gap-y-1 text-xs">
								<dt class="text-muted-foreground">Keys</dt>
								<dd class="font-mono">
									{idx.keySchema
										.map((k) => `${k.attributeName} (${k.keyType === 'HASH' ? 'PK' : 'SK'})`)
										.join(', ')}
								</dd>
								<dt class="text-muted-foreground">Items</dt>
								<dd class="font-mono">{idx.itemCount.toLocaleString()}</dd>
							</dl>
						</div>
					{/each}
				</div>
			</section>
		{/if}

		{#if detail.localSecondaryIndexes.length > 0}
			<section>
				<h3 class="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
					Local secondary indexes
				</h3>
				<div class="flex flex-col gap-3">
					{#each detail.localSecondaryIndexes as idx (idx.indexName)}
						<div class="rounded-md border border-border p-3">
							<div class="mb-2 flex items-center justify-between">
								<span class="font-mono text-sm">{idx.indexName}</span>
								<Badge variant="outline">{idx.projectionType}</Badge>
							</div>
							<dl class="grid grid-cols-[100px_1fr] gap-y-1 text-xs">
								<dt class="text-muted-foreground">Keys</dt>
								<dd class="font-mono">
									{idx.keySchema
										.map((k) => `${k.attributeName} (${k.keyType === 'HASH' ? 'PK' : 'SK'})`)
										.join(', ')}
								</dd>
							</dl>
						</div>
					{/each}
				</div>
			</section>
		{/if}
	{/if}
</div>

<CreateGsiDialog
	bind:open={addOpen}
	{detail}
	onClose={() => (addOpen = false)}
	onCreated={onUpdated}
/>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete GSI?"
	description={`Delete index "${deleteTarget ?? ''}" from ${detail.name}. Items are not affected.`}
	busy={deleting}
	onConfirm={confirmDelete}
	onClose={() => {
		deleteOpen = false;
		deleteTarget = null;
	}}
/>
