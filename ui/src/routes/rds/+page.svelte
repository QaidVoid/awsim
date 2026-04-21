<script lang="ts">
    import { onMount } from 'svelte';
    import { listDbInstances, createDbInstance, deleteDbInstance, listDbClusters } from '$lib/aws';

    let activeTab = $state<'instances' | 'clusters'>('instances');

    // ---- Instances ----
    interface DbInstance {
        identifier: string;
        engine: string;
        status: string;
        endpoint: string;
        instanceClass: string;
    }

    interface DbCluster {
        identifier: string;
        engine: string;
        status: string;
        endpoint: string;
    }

    let instances = $state<DbInstance[]>([]);
    let instancesLoading = $state(false);
    let instancesError = $state<string | null>(null);
    let showCreate = $state(false);
    let newId = $state('');
    let newEngine = $state('postgres');
    let newClass = $state('db.t3.micro');
    let creating = $state(false);
    let createError = $state<string | null>(null);
    let confirmDelete = $state<string | null>(null);

    // ---- Clusters ----
    let clusters = $state<DbCluster[]>([]);
    let clustersLoading = $state(false);
    let clustersError = $state<string | null>(null);

    function xmlValue(xml: string, tag: string): string {
        const m = xml.match(new RegExp(`<${tag}>([^<]*)<\/${tag}>`));
        return m ? m[1] : '';
    }

    function xmlArray(xml: string, itemTag: string, fields: string[]): Record<string, string>[] {
        const items: Record<string, string>[] = [];
        const regex = new RegExp(`<${itemTag}>([\\s\\S]*?)<\/${itemTag}>`, 'g');
        let match;
        while ((match = regex.exec(xml)) !== null) {
            const item: Record<string, string> = {};
            for (const field of fields) {
                item[field] = xmlValue(match[1], field);
            }
            items.push(item);
        }
        return items;
    }

    async function loadInstances() {
        instancesLoading = true;
        instancesError = null;
        try {
            const xml = await listDbInstances();
            const raw = xmlArray(xml, 'DBInstance', ['DBInstanceIdentifier', 'Engine', 'DBInstanceStatus', 'Address', 'DBInstanceClass']);
            instances = raw.map((r) => ({
                identifier: r['DBInstanceIdentifier'] ?? '',
                engine: r['Engine'] ?? '',
                status: r['DBInstanceStatus'] ?? '',
                endpoint: r['Address'] ?? '',
                instanceClass: r['DBInstanceClass'] ?? '',
            }));
        } catch (e) {
            instancesError = e instanceof Error ? e.message : 'Failed to load DB instances';
        } finally {
            instancesLoading = false;
        }
    }

    async function handleCreate() {
        if (!newId.trim()) return;
        creating = true;
        createError = null;
        try {
            await createDbInstance(newId.trim(), newEngine, newClass);
            newId = '';
            showCreate = false;
            await loadInstances();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed to create DB instance';
        } finally {
            creating = false;
        }
    }

    async function handleDelete(id: string) {
        try {
            await deleteDbInstance(id);
            confirmDelete = null;
            await loadInstances();
        } catch (e) {
            instancesError = e instanceof Error ? e.message : 'Failed to delete DB instance';
        }
    }

    async function loadClusters() {
        clustersLoading = true;
        clustersError = null;
        try {
            const xml = await listDbClusters();
            const raw = xmlArray(xml, 'DBCluster', ['DBClusterIdentifier', 'Engine', 'Status', 'Endpoint']);
            clusters = raw.map((r) => ({
                identifier: r['DBClusterIdentifier'] ?? '',
                engine: r['Engine'] ?? '',
                status: r['Status'] ?? '',
                endpoint: r['Endpoint'] ?? '',
            }));
        } catch (e) {
            clustersError = e instanceof Error ? e.message : 'Failed to load DB clusters';
        } finally {
            clustersLoading = false;
        }
    }

    function statusClass(status: string): string {
        const s = status.toLowerCase();
        if (s === 'available') return 'bg-green-900/40 text-green-400 border-green-800';
        if (s === 'creating' || s === 'modifying' || s === 'starting') return 'bg-yellow-900/40 text-yellow-400 border-yellow-800';
        if (s === 'deleting' || s === 'failed' || s === 'stopped') return 'bg-red-900/40 text-red-400 border-red-800';
        return 'bg-zinc-800 text-zinc-400 border-zinc-700';
    }

    function switchTab(tab: 'instances' | 'clusters') {
        activeTab = tab;
        if (tab === 'instances' && instances.length === 0 && !instancesLoading) loadInstances();
        if (tab === 'clusters' && clusters.length === 0 && !clustersLoading) loadClusters();
    }

    onMount(() => loadInstances());
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">RDS — Relational Database Service</h1>
            <p class="text-zinc-500 mt-1">Manage DB instances and clusters.</p>
        </div>
        {#if activeTab === 'instances'}
            <button
                onclick={() => { showCreate = !showCreate; createError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Instance
            </button>
        {/if}
    </div>

    <!-- Tabs -->
    <div class="flex gap-1 mb-4 border-b border-zinc-800">
        {#each ['instances', 'clusters'] as tab}
            <button
                onclick={() => switchTab(tab as 'instances' | 'clusters')}
                class="px-4 py-2 text-sm font-medium border-b-2 transition-colors {activeTab === tab ? 'border-orange-400 text-orange-400' : 'border-transparent text-zinc-500 hover:text-zinc-300'}"
            >
                {tab.charAt(0).toUpperCase() + tab.slice(1)}
            </button>
        {/each}
    </div>

    <!-- Instances tab -->
    {#if activeTab === 'instances'}
        {#if showCreate}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create DB Instance</h3>
                {#if createError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createError}</div>
                {/if}
                <div class="grid grid-cols-1 md:grid-cols-3 gap-3 mb-3">
                    <div>
                        <label for="db-id" class="block text-xs text-zinc-400 mb-1">DB Identifier</label>
                        <input
                            id="db-id"
                            type="text"
                            bind:value={newId}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                            placeholder="my-database"
                        />
                    </div>
                    <div>
                        <label for="db-engine" class="block text-xs text-zinc-400 mb-1">Engine</label>
                        <select
                            id="db-engine"
                            bind:value={newEngine}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        >
                            <option value="postgres">PostgreSQL</option>
                            <option value="mysql">MySQL</option>
                            <option value="mariadb">MariaDB</option>
                        </select>
                    </div>
                    <div>
                        <label for="db-class" class="block text-xs text-zinc-400 mb-1">Instance Class</label>
                        <select
                            id="db-class"
                            bind:value={newClass}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        >
                            <option value="db.t3.micro">db.t3.micro</option>
                            <option value="db.t3.small">db.t3.small</option>
                            <option value="db.t3.medium">db.t3.medium</option>
                            <option value="db.m5.large">db.m5.large</option>
                        </select>
                    </div>
                </div>
                <div class="flex gap-2">
                    <button
                        onclick={handleCreate}
                        disabled={creating || !newId.trim()}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                    >
                        {creating ? 'Creating...' : 'Create'}
                    </button>
                    <button
                        onclick={() => { showCreate = false; createError = null; newId = ''; }}
                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        {/if}

        {#if instancesLoading}
            <div class="text-zinc-500">Loading DB instances...</div>
        {:else if instancesError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{instancesError}</div>
        {:else if instances.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No DB instances yet.</p>
                <button onclick={() => showCreate = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                    Create your first instance
                </button>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Identifier</th>
                            <th class="px-4 py-3">Engine</th>
                            <th class="px-4 py-3">Status</th>
                            <th class="px-4 py-3">Endpoint</th>
                            <th class="px-4 py-3">Class</th>
                            <th class="px-4 py-3"></th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each instances as inst}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400">{inst.identifier}</td>
                                <td class="px-4 py-3 text-zinc-300">{inst.engine}</td>
                                <td class="px-4 py-3">
                                    <span class="inline-block px-2 py-0.5 rounded text-xs border {statusClass(inst.status)}">
                                        {inst.status || 'unknown'}
                                    </span>
                                </td>
                                <td class="px-4 py-3 text-zinc-400 text-xs font-mono">{inst.endpoint || '—'}</td>
                                <td class="px-4 py-3 text-zinc-400 text-xs">{inst.instanceClass}</td>
                                <td class="px-4 py-3">
                                    {#if confirmDelete === inst.identifier}
                                        <div class="flex items-center gap-1">
                                            <button onclick={() => handleDelete(inst.identifier)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDelete = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button onclick={() => confirmDelete = inst.identifier} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                    {/if}
                                </td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}

    <!-- Clusters tab -->
    {#if activeTab === 'clusters'}
        {#if clustersLoading}
            <div class="text-zinc-500">Loading DB clusters...</div>
        {:else if clustersError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{clustersError}</div>
        {:else if clusters.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No DB clusters yet.</p>
                <p class="text-zinc-600 text-xs mt-2">Create a cluster with the AWS CLI:</p>
                <code class="block mt-1 text-xs text-orange-400 font-mono">
                    aws --endpoint-url http://localhost:4566 rds create-db-cluster ...
                </code>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Identifier</th>
                            <th class="px-4 py-3">Engine</th>
                            <th class="px-4 py-3">Status</th>
                            <th class="px-4 py-3">Endpoint</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each clusters as cluster}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400">{cluster.identifier}</td>
                                <td class="px-4 py-3 text-zinc-300">{cluster.engine}</td>
                                <td class="px-4 py-3">
                                    <span class="inline-block px-2 py-0.5 rounded text-xs border {statusClass(cluster.status)}">
                                        {cluster.status || 'unknown'}
                                    </span>
                                </td>
                                <td class="px-4 py-3 text-zinc-400 text-xs font-mono">{cluster.endpoint || '—'}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}
</div>
