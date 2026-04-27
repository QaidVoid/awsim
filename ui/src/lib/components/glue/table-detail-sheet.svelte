<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription,
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { getTable, type GlueTable } from '$lib/api/glue';
	import { toast } from 'svelte-sonner';

	interface Props {
		open: boolean;
		databaseName: string | null;
		name: string | null;
		onOpenChange: (open: boolean) => void;
	}

	let { open, databaseName, name, onOpenChange }: Props = $props();

	let detail = $state<GlueTable | null>(null);
	let loading = $state(false);
	let lastKey = $state<string | null>(null);

	let key = $derived(databaseName && name ? `${databaseName}/${name}` : null);

	$effect(() => {
		if (open && key && key !== lastKey) {
			load();
		}
		if (!open) {
			detail = null;
			lastKey = null;
		}
	});

	async function load() {
		if (!databaseName || !name) return;
		loading = true;
		lastKey = key;
		try {
			detail = await getTable(databaseName, name);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load table');
		} finally {
			loading = false;
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-xl">
		<SheetHeader>
			<SheetTitle>Table</SheetTitle>
			<SheetDescription>
				{#if databaseName && name}
					<span class="font-mono text-xs">{databaseName}.{name}</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4">
			{#if loading}
				<p class="text-xs text-muted-foreground">Loading…</p>
			{:else if detail}
				<dl class="grid grid-cols-[140px_1fr] gap-x-3 gap-y-1.5 text-xs">
					<dt class="text-muted-foreground">Type</dt>
					<dd>
						{#if detail.tableType}
							<Badge variant="outline" class="h-4 px-1 text-[10px]">{detail.tableType}</Badge>
						{:else}—{/if}
					</dd>
					<dt class="text-muted-foreground">Owner</dt>
					<dd>{detail.owner || '—'}</dd>
					<dt class="text-muted-foreground">Storage location</dt>
					<dd class="font-mono break-all text-[11px]">{detail.storageLocation ?? '—'}</dd>
					<dt class="text-muted-foreground">Input format</dt>
					<dd class="font-mono break-all text-[11px]">{detail.inputFormat ?? '—'}</dd>
					<dt class="text-muted-foreground">Output format</dt>
					<dd class="font-mono break-all text-[11px]">{detail.outputFormat ?? '—'}</dd>
					<dt class="text-muted-foreground">SerDe</dt>
					<dd class="font-mono text-[11px]">{detail.serdeName ?? '—'}</dd>
					<dt class="text-muted-foreground">Created</dt>
					<dd>{detail.createTime ?? '—'}</dd>
					<dt class="text-muted-foreground">Updated</dt>
					<dd>{detail.updateTime ?? '—'}</dd>
				</dl>

				<section>
					<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">Columns</h3>
					{#if detail.columns.length === 0}
						<p class="text-xs text-muted-foreground">No columns.</p>
					{:else}
						<table class="w-full border-collapse text-xs">
							<thead>
								<tr class="border-b border-border text-left text-muted-foreground">
									<th class="py-1 pr-2">Name</th>
									<th class="py-1 pr-2">Type</th>
									<th class="py-1">Comment</th>
								</tr>
							</thead>
							<tbody>
								{#each detail.columns as c (c.name)}
									<tr class="border-b border-border/40">
										<td class="py-1 pr-2 font-mono">{c.name}</td>
										<td class="py-1 pr-2 font-mono text-muted-foreground">{c.type}</td>
										<td class="py-1 text-muted-foreground">{c.comment ?? ''}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					{/if}
				</section>

				{#if detail.partitionKeys.length > 0}
					<section>
						<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">
							Partition keys
						</h3>
						<div class="flex flex-wrap gap-1">
							{#each detail.partitionKeys as k (k.name)}
								<Badge variant="outline" class="h-4 px-1 text-[10px]">
									{k.name}: {k.type}
								</Badge>
							{/each}
						</div>
					</section>
				{/if}
			{/if}
		</div>

		<div class="flex justify-end gap-2 border-t border-border px-4 py-3">
			<Button variant="outline" onclick={() => onOpenChange(false)}>Close</Button>
		</div>
	</SheetContent>
</Sheet>
