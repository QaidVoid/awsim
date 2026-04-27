<script lang="ts">
	import { onMount } from 'svelte';
	import { listDataSources, type DataSource } from '$lib/api/appsync';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import DatabaseIcon from '@lucide/svelte/icons/database';
	import { toast } from 'svelte-sonner';

	interface Props {
		apiId: string;
	}

	let { apiId }: Props = $props();

	let rows = $state<DataSource[]>([]);
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
			rows = await listDataSources(apiId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load data sources');
		} finally {
			loading = false;
		}
	}

	function targetOf(d: DataSource): string {
		if (d.dynamodbConfig) return d.dynamodbConfig.tableName;
		if (d.lambdaConfig) return d.lambdaConfig.lambdaFunctionArn;
		if (d.httpConfig) return d.httpConfig.endpoint;
		return '—';
	}
</script>

{#snippet typeCell(row: DataSource)}
	<Badge variant="outline" class="font-mono text-[10px]">{row.type}</Badge>
{/snippet}

{#snippet targetCell(row: DataSource)}
	<span class="font-mono text-xs">{targetOf(row)}</span>
{/snippet}

{#snippet descCell(row: DataSource)}
	<span class="text-xs text-muted-foreground">{row.description ?? '—'}</span>
{/snippet}

<div class="p-4">
	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'type', label: 'Type', cell: typeCell },
			{ key: 'target', label: 'Target', cell: targetCell },
			{ key: 'description', label: 'Description', cell: descCell }
		]}
		rowKey={(r) => r.name}
	>
		{#snippet empty()}
			<EmptyState
				icon={DatabaseIcon}
				title="No data sources"
				description="Attach a DynamoDB table, Lambda, or HTTP endpoint to back resolvers."
			/>
		{/snippet}
	</DataTable>
</div>
