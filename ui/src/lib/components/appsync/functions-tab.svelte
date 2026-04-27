<script lang="ts">
	import { onMount } from 'svelte';
	import { listFunctions, type AppsyncFunction } from '$lib/api/appsync';
	import { DataTable, EmptyState } from '$lib/components/service';
	import FunctionSquareIcon from '@lucide/svelte/icons/function-square';
	import { toast } from 'svelte-sonner';

	interface Props {
		apiId: string;
	}

	let { apiId }: Props = $props();

	let rows = $state<AppsyncFunction[]>([]);
	let loading = $state(true);

	let lastApiId = $state<string | null>(null);
	$effect(() => {
		if (apiId !== lastApiId) {
			lastApiId = apiId;
			load();
		}
	});

	onMount(load);

	async function load() {
		loading = true;
		try {
			rows = await listFunctions(apiId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load functions');
		} finally {
			loading = false;
		}
	}
</script>

{#snippet descCell(row: AppsyncFunction)}
	<span class="text-xs text-muted-foreground">{row.description ?? '—'}</span>
{/snippet}

<div class="p-4">
	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'functionId', label: 'Function ID', mono: true },
			{ key: 'dataSourceName', label: 'Data source', mono: true },
			{ key: 'description', label: 'Description', cell: descCell }
		]}
		rowKey={(r) => r.functionId}
	>
		{#snippet empty()}
			<EmptyState
				icon={FunctionSquareIcon}
				title="No functions"
				description="Pipeline functions can be composed into a pipeline resolver."
			/>
		{/snippet}
	</DataTable>
</div>
