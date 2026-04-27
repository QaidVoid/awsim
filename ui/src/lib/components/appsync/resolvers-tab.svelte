<script lang="ts">
	import { onMount } from 'svelte';
	import { listAllRootResolvers, type Resolver } from '$lib/api/appsync';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import { toast } from 'svelte-sonner';

	interface Props {
		apiId: string;
	}

	let { apiId }: Props = $props();

	let rows = $state<Resolver[]>([]);
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
			rows = await listAllRootResolvers(apiId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load resolvers');
		} finally {
			loading = false;
		}
	}
</script>

{#snippet typeCell(row: Resolver)}
	<Badge variant="outline" class="font-mono text-[10px]">{row.typeName}</Badge>
{/snippet}

{#snippet kindCell(row: Resolver)}
	<Badge variant={row.kind === 'PIPELINE' ? 'secondary' : 'outline'} class="text-[10px]">
		{row.kind}
	</Badge>
{/snippet}

<div class="p-4">
	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'typeName', label: 'Type', cell: typeCell },
			{ key: 'fieldName', label: 'Field', mono: true },
			{ key: 'dataSourceName', label: 'Data source', mono: true },
			{ key: 'kind', label: 'Kind', cell: kindCell }
		]}
		rowKey={(r) => `${r.typeName}.${r.fieldName}`}
	>
		{#snippet empty()}
			<EmptyState
				icon={GitBranchIcon}
				title="No resolvers"
				description="Attach a resolver to a Query, Mutation, or Subscription field."
			/>
		{/snippet}
	</DataTable>
</div>
