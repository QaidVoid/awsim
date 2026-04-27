<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listNamedQueries,
		batchGetNamedQuery,
		type NamedQuery,
	} from '$lib/api/athena';
	import { DataTable, EmptyState } from '$lib/components/service';
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription,
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import BookmarkIcon from '@lucide/svelte/icons/bookmark';
	import { toast } from 'svelte-sonner';

	let rows = $state<NamedQuery[]>([]);
	let loading = $state(true);
	let sheetOpen = $state(false);
	let detail = $state<NamedQuery | null>(null);

	onMount(load);

	async function load() {
		loading = true;
		try {
			const ids = await listNamedQueries();
			rows = await batchGetNamedQuery(ids.slice(0, 50));
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load named queries');
		} finally {
			loading = false;
		}
	}

	function open(row: NamedQuery) {
		detail = row;
		sheetOpen = true;
	}
</script>

{#snippet descCell(row: NamedQuery)}
	<span class="line-clamp-1 text-xs text-muted-foreground">{row.description || '—'}</span>
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="text-xs text-muted-foreground">
			{rows.length} saved quer{rows.length === 1 ? 'y' : 'ies'}
		</div>
		<Button variant="outline" size="sm" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name' },
			{ key: 'database', label: 'Database' },
			{ key: 'workGroup', label: 'WG' },
			{ key: 'description', label: 'Description', cell: descCell },
		]}
		rowKey={(r) => r.namedQueryId}
		onRowClick={open}
	>
		{#snippet empty()}
			<EmptyState
				icon={BookmarkIcon}
				title="No named queries"
				description="Save frequently-used SQL statements as named queries."
			/>
		{/snippet}
	</DataTable>
</div>

<Sheet open={sheetOpen} onOpenChange={(o) => (sheetOpen = o)}>
	<SheetContent side="right" class="w-full sm:max-w-lg">
		<SheetHeader>
			<SheetTitle>Named query</SheetTitle>
			<SheetDescription>
				{#if detail}<span class="font-mono text-xs">{detail.namedQueryId}</span>{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4">
			{#if detail}
				<dl class="grid grid-cols-[120px_1fr] gap-x-3 gap-y-1.5 text-xs">
					<dt class="text-muted-foreground">Name</dt>
					<dd>{detail.name}</dd>
					<dt class="text-muted-foreground">Database</dt>
					<dd>{detail.database || '—'}</dd>
					<dt class="text-muted-foreground">WorkGroup</dt>
					<dd>{detail.workGroup}</dd>
					<dt class="text-muted-foreground">Description</dt>
					<dd>{detail.description || '—'}</dd>
				</dl>

				<section>
					<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">SQL</h3>
					<pre
						class="overflow-auto rounded-md border border-border bg-muted/40 p-3 text-xs whitespace-pre-wrap break-words">{detail.queryString}</pre>
				</section>
			{/if}
		</div>

		<div class="flex justify-end gap-2 border-t border-border px-4 py-3">
			<Button variant="outline" onclick={() => (sheetOpen = false)}>Close</Button>
		</div>
	</SheetContent>
</Sheet>
