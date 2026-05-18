<script lang="ts">
	import { onMount } from 'svelte';
	import { getDatabases, getTables, type GlueDatabase, type GlueTable } from '$lib/api/glue';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import TableIcon from '@lucide/svelte/icons/table';
	import { toast } from 'svelte-sonner';

	interface Props {
		onSelect?: (t: GlueTable) => void;
	}

	let { onSelect }: Props = $props();

	let databases = $state<GlueDatabase[]>([]);
	let database = $state<string>('');
	let tables = $state<GlueTable[]>([]);
	let loading = $state(true);

	onMount(async () => {
		await loadDatabases();
	});

	async function loadDatabases() {
		loading = true;
		try {
			databases = await getDatabases();
			if (!database && databases.length > 0) {
				database = databases[0].name;
				await loadTables();
			} else if (database) {
				await loadTables();
			} else {
				tables = [];
				loading = false;
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load databases');
			loading = false;
		}
	}

	async function loadTables() {
		if (!database) {
			tables = [];
			loading = false;
			return;
		}
		loading = true;
		try {
			tables = await getTables(database);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load tables');
			tables = [];
		} finally {
			loading = false;
		}
	}
</script>

{#snippet typeCell(row: GlueTable)}
	{#if row.tableType}
		<Badge variant="outline" class="h-4 px-1 text-[10px]">{row.tableType}</Badge>
	{:else}
		<span class="text-[10px] text-muted-foreground">—</span>
	{/if}
{/snippet}

{#snippet colsCell(row: GlueTable)}
	<span class="font-mono text-xs">{row.columns.length}</span>
{/snippet}

{#snippet locCell(row: GlueTable)}
	<span class="font-mono text-[10px]">{row.storageLocation ?? '—'}</span>
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex flex-wrap items-center gap-2">
		<Select
			type="single"
			bind:value={database}
			onValueChange={() => loadTables()}
		>
			<SelectTrigger
				aria-label="Select database"
				size="sm"
				class="w-[180px] text-xs"
			>
				{database || 'No databases'}
			</SelectTrigger>
			<SelectContent>
				{#each databases as db (db.name)}
					<SelectItem value={db.name} label={db.name}>{db.name}</SelectItem>
				{/each}
			</SelectContent>
		</Select>
		<div class="text-xs text-muted-foreground">
			{tables.length} table{tables.length === 1 ? '' : 's'}
		</div>
		<div class="ml-auto">
			<Button variant="outline" size="sm" onclick={loadTables} disabled={loading || !database}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
		</div>
	</div>

	<DataTable
		rows={tables}
		{loading}
		columns={[
			{ key: 'name', label: 'Name' },
			{ key: 'tableType', label: 'Type', cell: typeCell },
			{ key: 'columns', label: 'Cols', cell: colsCell },
			{ key: 'storageLocation', label: 'Location', cell: locCell },
			{ key: 'updateTime', label: 'Updated' },
		]}
		rowKey={(r) => `${r.databaseName}/${r.name}`}
		onRowClick={onSelect}
	>
		{#snippet empty()}
			<EmptyState
				icon={TableIcon}
				title="No tables"
				description="Tables describe schema and storage location for data the catalog tracks."
			/>
		{/snippet}
	</DataTable>
</div>
