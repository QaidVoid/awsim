<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { onMount } from 'svelte';
	import { pendingAction } from '$lib/pending-action.svelte';
	import { page } from '$app/state';
	import { afterNavigate, replaceState } from '$app/navigation';
	import { browser } from '$app/environment';
	import { toast } from 'svelte-sonner';
	import {
		listTables,
		describeTable,
		deleteTable,
		truncateTable,
		type TableSummary,
		type TableDetail,
		type Item
	} from '$lib/api/dynamodb';
	import { ResourceConsole } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import TableList from '$lib/components/dynamodb/table-list.svelte';
	import ItemsTab from '$lib/components/dynamodb/items-tab.svelte';
	import IndexesTab from '$lib/components/dynamodb/indexes-tab.svelte';
	import SchemaTab from '$lib/components/dynamodb/schema-tab.svelte';
	import PartiqlTab from '$lib/components/dynamodb/partiql-tab.svelte';
	import BackupsTab from '$lib/components/dynamodb/backups-tab.svelte';
	import ItemEditor from '$lib/components/dynamodb/item-editor.svelte';
	import CreateTableDialog from '$lib/components/dynamodb/create-table-dialog.svelte';
	import ImportTableDialog from '$lib/components/dynamodb/import-table-dialog.svelte';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import GlobalTablesDialog from '$lib/components/dynamodb/global-tables-dialog.svelte';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Eraser from '@lucide/svelte/icons/eraser';
	import Globe from '@lucide/svelte/icons/globe';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import Download from '@lucide/svelte/icons/download';

	let tables = $state<TableSummary[]>([]);
	let tablesLoading = $state(true);
	let filter = $state('');

	let selected = $state<TableSummary | null>(null);
	let detail = $state<TableDetail | null>(null);
	let detailLoading = $state(false);

	let createOpen = $state(false);
	let importOpen = $state(false);

	onMount(() => {
		if (pendingAction.consume('new-table')) createOpen = true;
	});
	let globalTablesOpen = $state(false);
	let confirmOpen = $state(false);
	let confirmBusy = $state(false);
	let truncateOpen = $state(false);
	let truncateBusy = $state(false);

	// Names of tables currently known to be deletion-protected. Lazy
	// to avoid an N+1 describe storm on first paint — populated
	// asynchronously after the table list arrives, with a cap to keep
	// large lists responsive.
	let protectedNames = $state<Set<string>>(new Set());
	const PREFETCH_DESCRIBE_CAP = 50;

	let editorOpen = $state(false);
	let editingItem = $state<Item | null>(null);

	let active: string = $state(
		useTab('dynamodb', ['items', 'partiql', 'indexes', 'schema', 'backups'] as const, 'items', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);

	let routerReady = $state(false);
	afterNavigate(() => {
		routerReady = true;
	});

	$effect(() => {
		if (!browser || !routerReady) return;
		const url = new URL(window.location.href);
		const name = selected?.name;
		if (url.searchParams.get('table') === name) return;
		if (name) url.searchParams.set('table', name);
		else url.searchParams.delete('table');
		replaceState(url.toString(), {});
	});

	onMount(() => {
		void loadTables().then(() => {
			const tableName = page.url.searchParams.get('table');
			if (tableName) {
				const t = tables.find((x) => x.name === tableName);
				if (t) void selectTable(t);
			}
		});
	});

	async function loadTables() {
		tablesLoading = true;
		try {
			tables = await listTables();
			void prefetchProtection(tables);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to list tables');
		} finally {
			tablesLoading = false;
		}
	}

	// Fan out describes in parallel to find which tables have
	// deletion protection enabled, so the sidebar can show a lock
	// icon without a per-row describe on click. Capped so a thousand-
	// table dev environment doesn't fire a thousand round-trips.
	async function prefetchProtection(list: TableSummary[]) {
		const next = new Set<string>();
		await Promise.all(
			list.slice(0, PREFETCH_DESCRIBE_CAP).map(async (t) => {
				try {
					const d = await describeTable(t.name);
					if (d.deletionProtectionEnabled) next.add(t.name);
				} catch {
					/* unreachable / restarted server — ignore for the badge */
				}
			})
		);
		protectedNames = next;
	}

	function syncProtection(name: string, enabled: boolean) {
		const next = new Set(protectedNames);
		if (enabled) next.add(name);
		else next.delete(name);
		protectedNames = next;
	}

	async function selectTable(t: TableSummary) {
		selected = t;
		detail = null;
		detailLoading = true;
		active = 'items';
		try {
			detail = await describeTable(t.name);
			syncProtection(detail.name, detail.deletionProtectionEnabled);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to describe table');
		} finally {
			detailLoading = false;
		}
	}

	async function refreshDetail() {
		if (!selected) return;
		try {
			detail = await describeTable(selected.name);
			syncProtection(detail.name, detail.deletionProtectionEnabled);
		} catch {
			/* swallow */
		}
	}

	function openEditor(item: Item | null) {
		editingItem = item;
		editorOpen = true;
	}

	async function onTableCreated(name: string) {
		await loadTables();
		const t = tables.find((x) => x.name === name);
		if (t) await selectTable(t);
	}

	async function confirmDelete() {
		if (!selected) return;
		confirmBusy = true;
		try {
			await deleteTable(selected.name);
			toast.success(`Deleted table ${selected.name}`);
			confirmOpen = false;
			selected = null;
			detail = null;
			await loadTables();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete table');
		} finally {
			confirmBusy = false;
		}
	}

	async function confirmTruncate() {
		if (!selected) return;
		truncateBusy = true;
		try {
			const removed = await truncateTable(selected.name);
			toast.success(
				removed === 0
					? `${selected.name} was already empty`
					: `Truncated ${selected.name} (${removed.toLocaleString()} item${removed === 1 ? '' : 's'} removed)`
			);
			truncateOpen = false;
			await refreshDetail();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to truncate table');
		} finally {
			truncateBusy = false;
		}
	}
</script>

<ResourceConsole
	title="DynamoDB"
	description="Managed NoSQL — tables, items, queries, PartiQL."
	hasSelection={!!selected}
	loading={detailLoading || !detail}
	emptyHint="Select a table to inspect."
>
	{#snippet actions()}
		<Badge variant="outline" class="font-mono">
			{tables.length} table{tables.length === 1 ? '' : 's'}
		</Badge>
		<Button variant="outline" size="sm" onclick={() => (globalTablesOpen = true)}>
			<Globe class="size-3.5" />
			Global tables
		</Button>
		<Button variant="outline" size="sm" onclick={() => (importOpen = true)}>
			<Download class="size-3.5" />
			Import from S3
		</Button>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<Plus class="size-3.5" />
			Create table
		</Button>
	{/snippet}

	{#snippet list()}
		<TableList
			{tables}
			selectedName={selected?.name ?? null}
			loading={tablesLoading}
			onSelect={selectTable}
			{protectedNames}
			bind:filter
		/>
	{/snippet}

	{#snippet detailHeader()}
		{#if selected && detail}
			<div
				class="flex shrink-0 items-center justify-between gap-2 border-b border-border bg-background/40 px-4 py-2"
			>
				<div class="flex items-center gap-2">
					<span class="text-xs font-medium text-muted-foreground">Table</span>
					<span class="font-mono text-sm">{selected.name}</span>
					<Badge variant={detail.status === 'ACTIVE' ? 'secondary' : 'outline'}>
						{detail.status || 'UNKNOWN'}
					</Badge>
					{#if detail.deletionProtectionEnabled}
						<Badge
							variant="outline"
							class="gap-1 text-amber-500"
							title="Deletion protection is on. Toggle off in the Schema tab, or use Delete to disable + delete in one step."
						>
							<ShieldCheck class="size-3" />
							Protected
						</Badge>
					{/if}
				</div>
				<div class="flex items-center gap-1">
					<Button variant="ghost" size="sm" onclick={() => (truncateOpen = true)}>
						<Eraser class="size-3.5" />
						Truncate
					</Button>
					<Button
						variant="ghost"
						size="sm"
						onclick={() => (confirmOpen = true)}
						disabled={detail.deletionProtectionEnabled}
						title={detail.deletionProtectionEnabled
							? 'Deletion protection is on. Disable it in the Schema tab to delete this table.'
							: undefined}
					>
						<Trash2 class="size-3.5 text-destructive" />
						Delete
					</Button>
				</div>
			</div>
		{/if}
	{/snippet}

	{#if selected && detail}
		<Tabs bind:value={active} class="flex min-h-0 min-w-0 flex-1 flex-col gap-0">
			<TabsList class="mx-4 mt-2 self-start">
				<TabsTrigger value="items">Items</TabsTrigger>
				<TabsTrigger value="partiql">PartiQL</TabsTrigger>
				<TabsTrigger value="indexes">Indexes</TabsTrigger>
				<TabsTrigger value="schema">Schema</TabsTrigger>
				<TabsTrigger value="backups">Backups</TabsTrigger>
			</TabsList>

			<div class="min-h-0 min-w-0 flex-1">
				<TabsContent value="items" class="m-0 h-full min-w-0">
					<ItemsTab {detail} onEdit={openEditor} />
				</TabsContent>
				<TabsContent value="partiql" class="m-0 h-full min-w-0">
					<PartiqlTab tableName={selected.name} />
				</TabsContent>
				<TabsContent value="indexes" class="m-0 h-full min-w-0">
					<IndexesTab {detail} onUpdated={refreshDetail} />
				</TabsContent>
				<TabsContent value="schema" class="m-0 h-full min-w-0">
					<SchemaTab {detail} onUpdated={refreshDetail} />
				</TabsContent>
				<TabsContent value="backups" class="m-0 h-full min-w-0">
					<BackupsTab
						{detail}
						onRestored={async (name) => {
							await loadTables();
							const t = tables.find((x) => x.name === name);
							if (t) await selectTable(t);
						}}
					/>
				</TabsContent>
			</div>
		</Tabs>
	{/if}
</ResourceConsole>

<CreateTableDialog
	bind:open={createOpen}
	onClose={() => (createOpen = false)}
	onCreated={onTableCreated}
/>

<ImportTableDialog
	bind:open={importOpen}
	onClose={() => (importOpen = false)}
	onImported={onTableCreated}
/>

<GlobalTablesDialog
	bind:open={globalTablesOpen}
	onClose={() => (globalTablesOpen = false)}
/>

<ItemEditor
	bind:open={editorOpen}
	{detail}
	item={editingItem}
	onClose={() => (editorOpen = false)}
	onSaved={refreshDetail}
/>

<ConfirmDialog
	bind:open={confirmOpen}
	title="Delete table?"
	description={`Permanently delete "${selected?.name ?? ''}" and all its items.`}
	busy={confirmBusy}
	onConfirm={confirmDelete}
	onClose={() => (confirmOpen = false)}
/>

<ConfirmDialog
	bind:open={truncateOpen}
	title="Truncate table?"
	description={`Delete every item in "${selected?.name ?? ''}". The schema, indexes, and stream config stay intact.`}
	confirmLabel="Truncate"
	busy={truncateBusy}
	onConfirm={confirmTruncate}
	onClose={() => (truncateOpen = false)}
/>
