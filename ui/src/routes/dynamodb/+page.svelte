<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listTables,
        createTable,
        deleteTable,
        describeTable,
        scanTable,
        putItem,
        deleteItem,
        type DynamoTable,
        type DynamoTableDetail,
        type DynamoAttributeValue,
    } from '$lib/aws';

    let tables = $state<DynamoTable[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let showCreateForm = $state(false);
    let newTableName = $state('');
    let newPartitionKey = $state('');
    let newPartitionKeyType = $state<'S' | 'N' | 'B'>('S');
    let newSortKey = $state('');
    let newSortKeyType = $state<'S' | 'N' | 'B'>('S');
    let creating = $state(false);
    let createError = $state<string | null>(null);

    let selectedTable = $state<string | null>(null);
    let tableDetail = $state<DynamoTableDetail | null>(null);
    let detailLoading = $state(false);

    let tableItems = $state<Record<string, DynamoAttributeValue>[]>([]);
    let itemsLoading = $state(false);
    let itemsError = $state<string | null>(null);

    let showPutItem = $state(false);
    let putItemJson = $state('{\n  "id": {"S": "example-id"},\n  "name": {"S": "example-name"}\n}');
    let putItemError = $state<string | null>(null);
    let puttingItem = $state(false);

    let confirmDeleteTable = $state<string | null>(null);

    let tableDetails = $state<Record<string, DynamoTableDetail>>({});

    async function loadTables() {
        loading = true;
        error = null;
        try {
            const data = await listTables();
            tables = data.tables;
            for (const t of data.tables) {
                describeTable(t.name).then((detail) => {
                    tableDetails = { ...tableDetails, [t.name]: detail };
                }).catch(() => {});
            }
        } catch {
            error = 'Could not connect to AWSim. Is it running on port 4566?';
        } finally {
            loading = false;
        }
    }

    async function handleCreateTable() {
        if (!newTableName.trim() || !newPartitionKey.trim()) return;
        creating = true;
        createError = null;
        try {
            await createTable(
                newTableName.trim(),
                newPartitionKey.trim(),
                newPartitionKeyType,
                newSortKey.trim() || undefined,
                newSortKeyType,
            );
            newTableName = '';
            newPartitionKey = '';
            newPartitionKeyType = 'S';
            newSortKey = '';
            newSortKeyType = 'S';
            showCreateForm = false;
            await loadTables();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed to create table';
        } finally {
            creating = false;
        }
    }

    async function handleDeleteTable(name: string) {
        try {
            await deleteTable(name);
            confirmDeleteTable = null;
            if (selectedTable === name) {
                selectedTable = null;
                tableDetail = null;
                tableItems = [];
            }
            await loadTables();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete table';
        }
    }

    async function selectTable(name: string) {
        selectedTable = name;
        tableDetail = null;
        tableItems = [];
        itemsError = null;
        detailLoading = true;
        showPutItem = false;
        putItemError = null;
        try {
            tableDetail = await describeTable(name);
        } catch {
            // not critical
        } finally {
            detailLoading = false;
        }
        await loadItems(name);
    }

    async function loadItems(name: string) {
        itemsLoading = true;
        itemsError = null;
        try {
            const res = await scanTable(name, 100);
            tableItems = res.items;
        } catch (e) {
            itemsError = e instanceof Error ? e.message : 'Failed to scan table';
        } finally {
            itemsLoading = false;
        }
    }

    async function handlePutItem() {
        if (!selectedTable) return;
        puttingItem = true;
        putItemError = null;
        try {
            const parsed = JSON.parse(putItemJson);
            await putItem(selectedTable, parsed);
            showPutItem = false;
            putItemJson = '{\n  "id": {"S": "example-id"},\n  "name": {"S": "example-name"}\n}';
            await loadItems(selectedTable);
            if (tableDetail) {
                describeTable(selectedTable).then((d) => { tableDetail = d; tableDetails = { ...tableDetails, [selectedTable!]: d }; }).catch(() => {});
            }
        } catch (e) {
            putItemError = e instanceof Error ? e.message : 'Failed to put item. Check JSON format.';
        } finally {
            puttingItem = false;
        }
    }

    async function handleDeleteItem(item: Record<string, DynamoAttributeValue>) {
        if (!selectedTable || !tableDetail) return;
        // Build key from key schema
        const key: Record<string, DynamoAttributeValue> = {};
        for (const k of tableDetail.keySchema) {
            if (item[k.attributeName] !== undefined) {
                key[k.attributeName] = item[k.attributeName];
            }
        }
        try {
            await deleteItem(selectedTable, key);
            tableItems = tableItems.filter((i) => i !== item);
            if (tableDetail) {
                describeTable(selectedTable).then((d) => { tableDetail = d; tableDetails = { ...tableDetails, [selectedTable!]: d }; }).catch(() => {});
            }
        } catch (e) {
            itemsError = e instanceof Error ? e.message : 'Failed to delete item';
        }
    }

    function dynoValueToString(val: DynamoAttributeValue): string {
        if (val.S !== undefined) return val.S;
        if (val.N !== undefined) return val.N;
        if (val.BOOL !== undefined) return String(val.BOOL);
        if (val.NULL) return 'null';
        if (val.L) return `[${val.L.map(dynoValueToString).join(', ')}]`;
        if (val.M) return JSON.stringify(Object.fromEntries(Object.entries(val.M).map(([k, v]) => [k, dynoValueToString(v)])));
        if (val.SS) return `{${val.SS.join(', ')}}`;
        if (val.NS) return `{${val.NS.join(', ')}}`;
        return JSON.stringify(val);
    }

    function dynoTypeLabel(val: DynamoAttributeValue): string {
        if (val.S !== undefined) return 'S';
        if (val.N !== undefined) return 'N';
        if (val.BOOL !== undefined) return 'BOOL';
        if (val.NULL) return 'NULL';
        if (val.L) return 'L';
        if (val.M) return 'M';
        if (val.SS) return 'SS';
        if (val.NS) return 'NS';
        return '?';
    }

    // All unique attribute keys across all items
    let allColumns = $derived(() => {
        const keys = new Set<string>();
        // Put key schema columns first if tableDetail is available
        if (tableDetail) {
            for (const k of tableDetail.keySchema) keys.add(k.attributeName);
        }
        for (const item of tableItems) {
            for (const k of Object.keys(item)) keys.add(k);
        }
        return Array.from(keys);
    });

    onMount(loadTables);
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">DynamoDB — Tables</h1>
            <p class="text-zinc-500 mt-1">Managed NoSQL database. Browse tables, items, and indexes.</p>
        </div>
        <div class="flex items-center gap-3">
            <span class="text-sm text-zinc-500">{tables.length} table{tables.length !== 1 ? 's' : ''}</span>
            <button
                onclick={() => { showCreateForm = !showCreateForm; createError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Table
            </button>
        </div>
    </div>

    {#if showCreateForm}
        <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
            <h3 class="font-semibold mb-4">Create Table</h3>
            {#if createError}
                <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createError}</div>
            {/if}
            <div class="flex flex-col gap-3">
                <div>
                    <label for="new-table-name" class="block text-xs text-zinc-500 mb-1">Table Name</label>
                    <input
                        id="new-table-name"
                        type="text"
                        bind:value={newTableName}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        placeholder="my-table"
                    />
                </div>
                <div>
                    <label for="new-pk-name" class="block text-xs text-zinc-500 mb-1">Partition Key (Hash Key)</label>
                    <div class="flex gap-2">
                        <input
                            id="new-pk-name"
                            type="text"
                            bind:value={newPartitionKey}
                            class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                            placeholder="id"
                        />
                        <select
                            bind:value={newPartitionKeyType}
                            class="bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        >
                            <option value="S">String (S)</option>
                            <option value="N">Number (N)</option>
                            <option value="B">Binary (B)</option>
                        </select>
                    </div>
                </div>
                <div>
                    <label for="new-sk-name" class="block text-xs text-zinc-500 mb-1">Sort Key (optional)</label>
                    <div class="flex gap-2">
                        <input
                            id="new-sk-name"
                            type="text"
                            bind:value={newSortKey}
                            class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                            placeholder="sk (leave blank if none)"
                        />
                        <select
                            bind:value={newSortKeyType}
                            class="bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        >
                            <option value="S">String (S)</option>
                            <option value="N">Number (N)</option>
                            <option value="B">Binary (B)</option>
                        </select>
                    </div>
                </div>
            </div>
            <div class="flex gap-2 mt-4">
                <button
                    onclick={handleCreateTable}
                    disabled={creating || !newTableName.trim() || !newPartitionKey.trim()}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                >
                    {creating ? 'Creating...' : 'Create'}
                </button>
                <button
                    onclick={() => { showCreateForm = false; createError = null; }}
                    class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                >
                    Cancel
                </button>
            </div>
        </div>
    {/if}

    {#if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400 mb-4">{error}</div>
    {/if}

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if tables.length === 0 && !showCreateForm}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No tables yet.</p>
            <button
                onclick={() => showCreateForm = true}
                class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium"
            >
                Create your first table
            </button>
        </div>
    {:else}
        <div class="flex gap-4">
            <!-- Table list -->
            <div class="w-72 shrink-0">
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                    {#each tables as table}
                        <div
                            class="border-b border-zinc-800/50 last:border-0 {selectedTable === table.name ? 'bg-zinc-800' : 'hover:bg-zinc-800/40'} cursor-pointer transition-colors"
                        >
                            <div class="px-4 py-3 flex items-start justify-between gap-2">
                                <button class="flex-1 text-left min-w-0" onclick={() => selectTable(table.name)}>
                                    <div class="font-mono text-orange-400 text-sm truncate">{table.name}</div>
                                    {#if tableDetails[table.name]}
                                        {@const detail = tableDetails[table.name]}
                                        <div class="text-xs text-zinc-500 mt-0.5">
                                            {detail.itemCount} item{detail.itemCount !== 1 ? 's' : ''}
                                            <span class="ml-1 px-1 rounded text-zinc-600 {detail.status === 'ACTIVE' ? 'text-green-600' : ''}">
                                                {detail.status}
                                            </span>
                                        </div>
                                        <div class="text-xs text-zinc-600 mt-0.5">
                                            {detail.keySchema.map((k) => `${k.attributeName} (${k.keyType === 'HASH' ? 'PK' : 'SK'})`).join(', ')}
                                        </div>
                                    {/if}
                                </button>
                                {#if confirmDeleteTable === table.name}
                                    <div class="flex flex-col gap-1 shrink-0">
                                        <button
                                            onclick={() => handleDeleteTable(table.name)}
                                            class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs"
                                        >
                                            Confirm
                                        </button>
                                        <button
                                            onclick={() => confirmDeleteTable = null}
                                            class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs"
                                        >
                                            Cancel
                                        </button>
                                    </div>
                                {:else}
                                    <button
                                        onclick={(e) => { e.stopPropagation(); confirmDeleteTable = table.name; }}
                                        class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs shrink-0 transition-colors"
                                    >
                                        Delete
                                    </button>
                                {/if}
                            </div>
                        </div>
                    {/each}
                </div>
            </div>

            <!-- Table detail panel -->
            <div class="flex-1 min-w-0 flex flex-col gap-4">
                {#if selectedTable}
                    <!-- Table info -->
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4">
                        <div class="flex items-center justify-between mb-3">
                            <h3 class="font-semibold text-zinc-200">{selectedTable}</h3>
                            {#if tableDetail}
                                <span class="text-xs px-2 py-0.5 rounded {tableDetail.status === 'ACTIVE' ? 'bg-green-900/40 text-green-400' : 'bg-zinc-700 text-zinc-400'}">
                                    {tableDetail.status}
                                </span>
                            {/if}
                        </div>
                        {#if detailLoading}
                            <div class="text-zinc-500 text-sm">Loading details...</div>
                        {:else if tableDetail}
                            <div class="grid grid-cols-2 gap-x-6 gap-y-1 text-sm">
                                <div class="text-zinc-500">Item count</div>
                                <div class="text-zinc-200">{tableDetail.itemCount}</div>
                                <div class="text-zinc-500">Table size</div>
                                <div class="text-zinc-200">{tableDetail.tableSizeBytes} bytes</div>
                                <div class="text-zinc-500">Key schema</div>
                                <div class="text-zinc-200">
                                    {tableDetail.keySchema.map((k) => `${k.attributeName} (${k.keyType === 'HASH' ? 'Partition' : 'Sort'})`).join(', ')}
                                </div>
                                {#if tableDetail.creationDateTime}
                                    <div class="text-zinc-500">Created</div>
                                    <div class="text-zinc-200 text-xs">{tableDetail.creationDateTime}</div>
                                {/if}
                            </div>
                        {/if}
                    </div>

                    <!-- Put item -->
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4">
                        <div class="flex items-center justify-between mb-3">
                            <h3 class="font-semibold text-zinc-200">Items</h3>
                            <div class="flex gap-2">
                                <button
                                    onclick={() => selectedTable && loadItems(selectedTable)}
                                    class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                                >
                                    Refresh
                                </button>
                                <button
                                    onclick={() => { showPutItem = !showPutItem; putItemError = null; }}
                                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
                                >
                                    Put Item
                                </button>
                            </div>
                        </div>

                        {#if showPutItem}
                            <div class="bg-zinc-800 rounded-lg border border-zinc-700 p-3 mb-4">
                                <h4 class="text-sm font-medium text-zinc-300 mb-2">Put Item (DynamoDB JSON)</h4>
                                {#if putItemError}
                                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs mb-2">{putItemError}</div>
                                {/if}
                                <textarea
                                    bind:value={putItemJson}
                                    rows={8}
                                    class="w-full bg-zinc-900 border border-zinc-700 rounded px-3 py-2 text-xs font-mono focus:outline-none focus:border-orange-500 resize-y"
                                ></textarea>
                                <div class="flex gap-2 mt-2">
                                    <button
                                        onclick={handlePutItem}
                                        disabled={puttingItem}
                                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm font-medium transition-colors"
                                    >
                                        {puttingItem ? 'Saving...' : 'Save Item'}
                                    </button>
                                    <button
                                        onclick={() => { showPutItem = false; putItemError = null; }}
                                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                                    >
                                        Cancel
                                    </button>
                                </div>
                            </div>
                        {/if}

                        {#if itemsError}
                            <div class="bg-red-900/20 border border-red-800 rounded p-3 text-red-400 text-sm">{itemsError}</div>
                        {:else if itemsLoading}
                            <div class="text-zinc-500 text-sm">Scanning table...</div>
                        {:else if tableItems.length === 0}
                            <div class="text-zinc-500 text-sm py-4 text-center">No items found. Use "Put Item" to add one.</div>
                        {:else}
                            <div class="overflow-x-auto">
                                <table class="w-full text-xs min-w-max">
                                    <thead>
                                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                                            {#each allColumns() as col}
                                                <th class="px-3 py-2 whitespace-nowrap font-medium">
                                                    {col}
                                                    {#if tableDetail?.keySchema.find((k) => k.attributeName === col)}
                                                        <span class="ml-1 text-orange-500/60 text-xs">
                                                            {tableDetail?.keySchema.find((k) => k.attributeName === col)?.keyType === 'HASH' ? 'PK' : 'SK'}
                                                        </span>
                                                    {/if}
                                                </th>
                                            {/each}
                                            <th class="px-3 py-2"></th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {#each tableItems as item}
                                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                                {#each allColumns() as col}
                                                    <td class="px-3 py-2 font-mono">
                                                        {#if item[col] !== undefined}
                                                            <span class="text-zinc-200">{dynoValueToString(item[col])}</span>
                                                            <span class="ml-1 text-zinc-600 text-xs">{dynoTypeLabel(item[col])}</span>
                                                        {:else}
                                                            <span class="text-zinc-700">—</span>
                                                        {/if}
                                                    </td>
                                                {/each}
                                                <td class="px-3 py-2">
                                                    <button
                                                        onclick={() => handleDeleteItem(item)}
                                                        class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors"
                                                    >
                                                        Delete
                                                    </button>
                                                </td>
                                            </tr>
                                        {/each}
                                    </tbody>
                                </table>
                            </div>
                            <div class="mt-2 text-xs text-zinc-600">{tableItems.length} item{tableItems.length !== 1 ? 's' : ''} (scan limit: 100)</div>
                        {/if}
                    </div>
                {:else}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-500 text-sm">
                        Select a table to browse items.
                    </div>
                {/if}
            </div>
        </div>
    {/if}
</div>
