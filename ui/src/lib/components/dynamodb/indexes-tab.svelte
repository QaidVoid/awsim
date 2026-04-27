<script lang="ts">
	import type { TableDetail } from '$lib/api/dynamodb';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import Layers from '@lucide/svelte/icons/layers';

	interface Props {
		detail: TableDetail;
	}

	let { detail }: Props = $props();

	let hasIndexes = $derived(
		detail.globalSecondaryIndexes.length > 0 || detail.localSecondaryIndexes.length > 0
	);
</script>

<div class="flex h-full min-h-0 flex-col gap-4 overflow-y-auto p-4">
	{#if !hasIndexes}
		<div class="flex h-full items-center justify-center">
			<EmptyState
				icon={Layers}
				title="No secondary indexes"
				description="This table only has its primary key."
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
							<div class="mb-2 flex items-center justify-between">
								<span class="font-mono text-sm">{idx.indexName}</span>
								<div class="flex gap-1.5">
									<Badge variant="outline">{idx.projectionType}</Badge>
									<Badge variant={idx.status === 'ACTIVE' ? 'secondary' : 'outline'}>
										{idx.status || 'UNKNOWN'}
									</Badge>
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
