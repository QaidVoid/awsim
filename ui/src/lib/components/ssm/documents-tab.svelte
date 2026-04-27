<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { DataTable, EmptyState, ListSkeleton } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import { toast } from 'svelte-sonner';
	import { listDocuments, getDocument, type SsmDocument, type SsmDocumentDetail } from '$lib/api/ssm';

	let docs = $state<SsmDocument[]>([]);
	let loading = $state(false);
	let detail = $state<SsmDocumentDetail | null>(null);
	let detailLoading = $state(false);
	let sheetOpen = $state(false);

	async function load() {
		loading = true;
		try {
			docs = await listDocuments();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load documents');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
	});

	async function open(d: SsmDocument) {
		sheetOpen = true;
		detail = null;
		detailLoading = true;
		try {
			detail = await getDocument(d.name);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load document');
		} finally {
			detailLoading = false;
		}
	}

	function platformsLabel(p?: string[]): string {
		return p && p.length > 0 ? p.join(', ') : '—';
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">SSM documents ({docs.length})</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	{#snippet typeCell(d: SsmDocument)}
		<Badge variant="outline" class="h-4 px-1.5 font-mono text-[10px]">
			{d.documentType ?? '—'}
		</Badge>
	{/snippet}

	{#snippet platformsCell(d: SsmDocument)}
		<span class="text-xs text-muted-foreground">{platformsLabel(d.platformTypes)}</span>
	{/snippet}

	<DataTable
		rows={docs}
		{loading}
		rowKey={(d) => d.name}
		onRowClick={open}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'documentType', label: 'Type', width: '140px', cell: typeCell },
			{ key: 'documentFormat', label: 'Format', width: '100px' },
			{ key: 'platformTypes', label: 'Platforms', width: '180px', cell: platformsCell },
			{ key: 'owner', label: 'Owner', width: '140px' }
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={FileTextIcon}
				title="No documents"
				description="SSM documents define automation, command, or session managers."
			/>
		{/snippet}
	</DataTable>
</div>

<Sheet bind:open={sheetOpen} onOpenChange={(o) => (sheetOpen = o)}>
	<SheetContent side="right" class="w-full max-w-2xl overflow-y-auto sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle>{detail?.name ?? 'Document'}</SheetTitle>
			<SheetDescription>
				{detail?.documentType ?? ''} · {detail?.documentFormat ?? ''}
			</SheetDescription>
		</SheetHeader>
		<div class="px-6 pb-6">
			{#if detailLoading}
				<ListSkeleton rows={4} />
			{:else if detail?.content}
				<pre
					class="max-h-[70vh] overflow-auto rounded-md border border-border bg-muted/40 p-3 font-mono text-[11px] whitespace-pre-wrap break-all">{detail.content}</pre>
			{:else}
				<p class="text-xs text-muted-foreground">No content available.</p>
			{/if}
		</div>
	</SheetContent>
</Sheet>
