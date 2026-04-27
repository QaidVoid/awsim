<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listWorkGroups,
		getWorkGroup,
		type WorkGroup,
		type WorkGroupDetail,
	} from '$lib/api/athena';
	import { DataTable, EmptyState } from '$lib/components/service';
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription,
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import LayoutGridIcon from '@lucide/svelte/icons/layout-grid';
	import { toast } from 'svelte-sonner';

	let rows = $state<WorkGroup[]>([]);
	let loading = $state(true);
	let sheetOpen = $state(false);
	let detail = $state<WorkGroupDetail | null>(null);
	let detailLoading = $state(false);

	onMount(load);

	async function load() {
		loading = true;
		try {
			rows = await listWorkGroups();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load workgroups');
		} finally {
			loading = false;
		}
	}

	async function open(row: WorkGroup) {
		detail = null;
		sheetOpen = true;
		detailLoading = true;
		try {
			detail = await getWorkGroup(row.name);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load workgroup');
		} finally {
			detailLoading = false;
		}
	}

	function stateVariant(s: string): 'secondary' | 'destructive' | 'outline' {
		if (s === 'ENABLED') return 'secondary';
		if (s === 'DISABLED') return 'destructive';
		return 'outline';
	}
</script>

{#snippet stateCell(row: WorkGroup)}
	<Badge variant={stateVariant(row.state)} class="h-4 px-1 text-[10px]">
		{row.state || '—'}
	</Badge>
{/snippet}

{#snippet descCell(row: WorkGroup)}
	<span class="line-clamp-1 text-xs text-muted-foreground">{row.description || '—'}</span>
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="text-xs text-muted-foreground">
			{rows.length} workgroup{rows.length === 1 ? '' : 's'}
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
			{ key: 'state', label: 'State', cell: stateCell },
			{ key: 'description', label: 'Description', cell: descCell },
			{ key: 'creationTime', label: 'Created' },
		]}
		rowKey={(r) => r.name}
		onRowClick={open}
	>
		{#snippet empty()}
			<EmptyState
				icon={LayoutGridIcon}
				title="No workgroups"
				description="Workgroups isolate users, queries, and result locations."
			/>
		{/snippet}
	</DataTable>
</div>

<Sheet open={sheetOpen} onOpenChange={(o) => (sheetOpen = o)}>
	<SheetContent side="right" class="w-full sm:max-w-lg">
		<SheetHeader>
			<SheetTitle>WorkGroup</SheetTitle>
			<SheetDescription>
				{#if detail}<span class="font-mono text-xs">{detail.name}</span>{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4">
			{#if detailLoading}
				<p class="text-xs text-muted-foreground">Loading…</p>
			{:else if detail}
				<dl class="grid grid-cols-[180px_1fr] gap-x-3 gap-y-1.5 text-xs">
					<dt class="text-muted-foreground">State</dt>
					<dd>
						<Badge variant={stateVariant(detail.state)} class="h-4 px-1 text-[10px]">
							{detail.state || '—'}
						</Badge>
					</dd>
					<dt class="text-muted-foreground">Description</dt>
					<dd>{detail.description || '—'}</dd>
					<dt class="text-muted-foreground">Created</dt>
					<dd>{detail.creationTime ?? '—'}</dd>
					<dt class="text-muted-foreground">Output location</dt>
					<dd class="font-mono break-all">{detail.outputLocation ?? '—'}</dd>
					<dt class="text-muted-foreground">Enforce config</dt>
					<dd>{detail.enforceWorkGroupConfiguration ? 'yes' : 'no'}</dd>
					<dt class="text-muted-foreground">CW metrics</dt>
					<dd>{detail.publishCloudWatchMetricsEnabled ? 'enabled' : 'disabled'}</dd>
					<dt class="text-muted-foreground">Bytes scanned cutoff</dt>
					<dd>{detail.bytesScannedCutoffPerQuery ?? '—'}</dd>
				</dl>
			{/if}
		</div>

		<div class="flex justify-end gap-2 border-t border-border px-4 py-3">
			<Button variant="outline" onclick={() => (sheetOpen = false)}>Close</Button>
		</div>
	</SheetContent>
</Sheet>
