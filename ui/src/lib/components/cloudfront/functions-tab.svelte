<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import FunctionSquareIcon from '@lucide/svelte/icons/function-square';
	import { toast } from 'svelte-sonner';
	import { listFunctions, type CloudFrontFunction } from '$lib/api/cloudfront';

	let functions = $state<CloudFrontFunction[]>([]);
	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			functions = await listFunctions();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load functions');
			functions = [];
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		void load();
	});
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Functions
			<span class="ml-1 font-normal text-muted-foreground">({functions.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		rows={functions}
		{loading}
		rowKey={(f) => `${f.name}-${f.stage}`}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'runtime', label: 'Runtime', width: '160px', cell: runtimeCell },
			{ key: 'stage', label: 'Stage', width: '120px', cell: stageCell },
			{ key: 'status', label: 'Status', width: '140px' },
			{ key: 'lastModifiedTime', label: 'Last modified', width: '200px' },
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={FunctionSquareIcon}
				title="No CloudFront functions"
				description="Lightweight JS functions running at the edge for header / URL rewriting."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet runtimeCell(f: CloudFrontFunction)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">{f.runtime || '—'}</Badge>
{/snippet}

{#snippet stageCell(f: CloudFrontFunction)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] uppercase">{f.stage || '—'}</Badge>
{/snippet}
