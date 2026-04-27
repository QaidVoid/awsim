<script lang="ts">
	import { onMount } from 'svelte';
	import { listKnowledgeBases, type KnowledgeBase } from '$lib/api/bedrock';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import LibraryIcon from '@lucide/svelte/icons/library';
	import { toast } from 'svelte-sonner';

	interface Props {
		onSelect: (kb: KnowledgeBase) => void;
	}

	let { onSelect }: Props = $props();

	let rows = $state<KnowledgeBase[]>([]);
	let loading = $state(true);

	onMount(load);

	async function load() {
		loading = true;
		try {
			rows = await listKnowledgeBases();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load knowledge bases');
		} finally {
			loading = false;
		}
	}

	function statusVariant(s: string): 'secondary' | 'destructive' | 'outline' {
		if (s === 'ACTIVE') return 'secondary';
		if (s === 'FAILED' || s === 'DELETING') return 'destructive';
		return 'outline';
	}
</script>

{#snippet statusCell(row: KnowledgeBase)}
	<Badge variant={statusVariant(row.status)} class="text-[10px]">{row.status || '—'}</Badge>
{/snippet}

{#snippet descCell(row: KnowledgeBase)}
	<span class="line-clamp-1 text-xs text-muted-foreground">{row.description ?? '—'}</span>
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="text-xs text-muted-foreground">
			{rows.length} knowledge base{rows.length === 1 ? '' : 's'}
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
			{ key: 'knowledgeBaseId', label: 'ID', mono: true },
			{ key: 'status', label: 'Status', cell: statusCell },
			{ key: 'description', label: 'Description', cell: descCell },
		]}
		rowKey={(r) => r.knowledgeBaseId}
		onRowClick={onSelect}
	>
		{#snippet empty()}
			<EmptyState
				icon={LibraryIcon}
				title="No knowledge bases"
				description="Knowledge bases let foundation models retrieve grounded responses from your data."
			/>
		{/snippet}
	</DataTable>
</div>
