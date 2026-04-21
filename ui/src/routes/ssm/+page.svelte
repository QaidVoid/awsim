<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listParameters, getParameter, putParameter, deleteParameter,
        type SsmParameter, type SsmParameterValue,
    } from '$lib/aws';

    let params = $state<SsmParameter[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let selectedParam = $state<SsmParameterValue | null>(null);
    let detailLoading = $state(false);
    let detailError = $state<string | null>(null);

    let confirmDelete = $state<string | null>(null);

    // Put parameter form
    let showPutForm = $state(false);
    let putName = $state('');
    let putValue = $state('');
    let putType = $state<'String' | 'StringList' | 'SecureString'>('String');
    let putting = $state(false);
    let putError = $state<string | null>(null);

    // Hierarchical folder state (expanded path prefixes)
    let expandedPaths = $state<Set<string>>(new Set());

    interface TreeNode {
        type: 'folder' | 'param';
        label: string;
        path: string;
        children?: TreeNode[];
        param?: SsmParameter;
    }

    let tree = $derived(() => buildTree(params));

    function buildTree(parameters: SsmParameter[]): TreeNode[] {
        const root: TreeNode[] = [];

        for (const param of parameters) {
            const parts = param.name.split('/').filter(Boolean);
            if (parts.length <= 1) {
                // Top-level non-hierarchical param
                root.push({ type: 'param', label: param.name, path: param.name, param });
                continue;
            }
            // Hierarchical — build folder nodes
            let nodes = root;
            let built = '';
            for (let i = 0; i < parts.length - 1; i++) {
                built += '/' + parts[i];
                let folder = nodes.find((n) => n.type === 'folder' && n.path === built);
                if (!folder) {
                    folder = { type: 'folder', label: parts[i], path: built, children: [] };
                    nodes.push(folder);
                }
                nodes = folder.children!;
            }
            const leafLabel = parts[parts.length - 1];
            nodes.push({ type: 'param', label: leafLabel, path: param.name, param });
        }

        return root;
    }

    function toggleFolder(path: string) {
        const next = new Set(expandedPaths);
        if (next.has(path)) {
            next.delete(path);
        } else {
            next.add(path);
        }
        expandedPaths = next;
    }

    async function loadParameters() {
        loading = true;
        error = null;
        try {
            const data = await listParameters();
            params = data.parameters;
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to load parameters';
        } finally {
            loading = false;
        }
    }

    async function selectParam(name: string) {
        detailLoading = true;
        detailError = null;
        selectedParam = null;
        try {
            selectedParam = await getParameter(name);
        } catch (e) {
            detailError = e instanceof Error ? e.message : 'Failed to get parameter';
        } finally {
            detailLoading = false;
        }
    }

    async function handleDelete(name: string) {
        try {
            await deleteParameter(name);
            confirmDelete = null;
            if (selectedParam?.name === name) selectedParam = null;
            await loadParameters();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete parameter';
        }
    }

    async function handlePut() {
        if (!putName.trim() || !putValue.trim()) return;
        putting = true;
        putError = null;
        try {
            await putParameter(putName.trim(), putValue, putType);
            putName = '';
            putValue = '';
            putType = 'String';
            showPutForm = false;
            await loadParameters();
        } catch (e) {
            putError = e instanceof Error ? e.message : 'Failed to put parameter';
        } finally {
            putting = false;
        }
    }

    function formatDate(iso: string): string {
        try { return new Date(iso).toLocaleString(); } catch { return iso; }
    }

    function typeColor(type: string): string {
        if (type === 'SecureString') return 'text-yellow-400';
        if (type === 'StringList') return 'text-blue-400';
        return 'text-zinc-400';
    }

    onMount(loadParameters);
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">SSM — Parameter Store</h1>
            <p class="text-zinc-500 mt-1">Store configuration and secrets hierarchically.</p>
        </div>
        <button
            onclick={() => { showPutForm = !showPutForm; putError = null; }}
            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
        >
            Put Parameter
        </button>
    </div>

    {#if showPutForm}
        <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
            <h3 class="font-semibold mb-3">Put Parameter</h3>
            {#if putError}
                <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{putError}</div>
            {/if}
            <div class="grid grid-cols-3 gap-3 mb-3">
                <div class="col-span-2">
                    <label for="param-name" class="block text-xs text-zinc-400 mb-1">Name</label>
                    <input
                        id="param-name"
                        type="text"
                        bind:value={putName}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500"
                        placeholder="/app/prod/db-url"
                    />
                </div>
                <div>
                    <label for="param-type" class="block text-xs text-zinc-400 mb-1">Type</label>
                    <select
                        id="param-type"
                        bind:value={putType}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                    >
                        <option value="String">String</option>
                        <option value="StringList">StringList</option>
                        <option value="SecureString">SecureString</option>
                    </select>
                </div>
            </div>
            <div class="mb-3">
                <label for="param-value" class="block text-xs text-zinc-400 mb-1">Value</label>
                <textarea
                    id="param-value"
                    bind:value={putValue}
                    rows="3"
                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y"
                    placeholder="parameter value..."
                ></textarea>
            </div>
            <div class="flex gap-2">
                <button
                    onclick={handlePut}
                    disabled={putting || !putName.trim() || !putValue.trim()}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                >
                    {putting ? 'Saving...' : 'Save'}
                </button>
                <button
                    onclick={() => { showPutForm = false; putError = null; putName = ''; putValue = ''; }}
                    class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                >
                    Cancel
                </button>
            </div>
        </div>
    {/if}

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else}
        <div class="flex gap-4">
            <!-- Left: tree view + table -->
            <div class="flex-1 min-w-0">
                {#if params.length === 0}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                        <p class="text-zinc-500">No parameters yet.</p>
                        <button onclick={() => showPutForm = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                            Add your first parameter
                        </button>
                    </div>
                {:else}
                    <!-- Flat table -->
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                        <table class="w-full text-sm">
                            <thead>
                                <tr class="border-b border-zinc-800 text-left text-zinc-500">
                                    <th class="px-4 py-3">Name</th>
                                    <th class="px-4 py-3">Type</th>
                                    <th class="px-4 py-3">Version</th>
                                    <th class="px-4 py-3">Last Modified</th>
                                    <th class="px-4 py-3"></th>
                                </tr>
                            </thead>
                            <tbody>
                                {#each params as param}
                                    <tr
                                        class="border-b border-zinc-800/50 hover:bg-zinc-800/30 cursor-pointer {selectedParam?.name === param.name ? 'bg-zinc-800/50' : ''}"
                                        onclick={() => selectParam(param.name)}
                                    >
                                        <td class="px-4 py-3 font-mono text-orange-400 text-xs">{param.name}</td>
                                        <td class="px-4 py-3 text-xs {typeColor(param.type)}">{param.type}</td>
                                        <td class="px-4 py-3 text-zinc-400 text-xs">{param.version}</td>
                                        <td class="px-4 py-3 text-zinc-400 text-xs">{param.lastModifiedDate ? formatDate(param.lastModifiedDate) : '—'}</td>
                                        <td class="px-4 py-3">
                                            {#if confirmDelete === param.name}
                                                <div class="flex items-center gap-1">
                                                    <button
                                                        onclick={(e) => { e.stopPropagation(); handleDelete(param.name); }}
                                                        class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs"
                                                    >Confirm</button>
                                                    <button
                                                        onclick={(e) => { e.stopPropagation(); confirmDelete = null; }}
                                                        class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs"
                                                    >Cancel</button>
                                                </div>
                                            {:else}
                                                <button
                                                    onclick={(e) => { e.stopPropagation(); confirmDelete = param.name; }}
                                                    class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors"
                                                >Delete</button>
                                            {/if}
                                        </td>
                                    </tr>
                                {/each}
                            </tbody>
                        </table>
                    </div>

                    <!-- Hierarchical view -->
                    <div class="mt-4">
                        <div class="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2 px-1">Hierarchy View</div>
                        <div class="bg-zinc-900 border border-zinc-800 rounded-lg p-3">
                            {#snippet renderNodes(nodes: TreeNode[], depth: number)}
                                {#each nodes as node}
                                    {#if node.type === 'folder'}
                                        <div style="padding-left: {depth * 16}px">
                                            <button
                                                onclick={() => toggleFolder(node.path)}
                                                class="flex items-center gap-1 text-sm text-zinc-400 hover:text-zinc-200 py-0.5 transition-colors"
                                            >
                                                <span class="text-zinc-600 text-xs">{expandedPaths.has(node.path) ? 'v' : '>'}</span>
                                                <span class="font-mono">{node.label}/</span>
                                                <span class="text-zinc-600 text-xs ml-1">({node.children?.length ?? 0})</span>
                                            </button>
                                            {#if expandedPaths.has(node.path) && node.children}
                                                {@render renderNodes(node.children, depth + 1)}
                                            {/if}
                                        </div>
                                    {:else}
                                        <div style="padding-left: {depth * 16}px">
                                            <button
                                                onclick={() => selectParam(node.path)}
                                                class="flex items-center gap-2 text-sm py-0.5 hover:text-orange-300 transition-colors w-full text-left {selectedParam?.name === node.path ? 'text-orange-400' : 'text-zinc-300'}"
                                            >
                                                <span class="text-zinc-600 text-xs shrink-0">-</span>
                                                <span class="font-mono text-xs">{node.label}</span>
                                                <span class="text-xs {typeColor(node.param?.type ?? '')} shrink-0">{node.param?.type}</span>
                                            </button>
                                        </div>
                                    {/if}
                                {/each}
                            {/snippet}
                            {@render renderNodes(tree(), 0)}
                        </div>
                    </div>
                {/if}
            </div>

            <!-- Right: detail panel -->
            <div class="w-72 shrink-0">
                <div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4 sticky top-4">
                    <div class="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-3">Parameter Detail</div>
                    {#if detailLoading}
                        <div class="text-zinc-500 text-sm">Loading...</div>
                    {:else if detailError}
                        <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm">{detailError}</div>
                    {:else if selectedParam}
                        <div class="space-y-3">
                            <div>
                                <div class="text-xs text-zinc-500 mb-0.5">Name</div>
                                <div class="font-mono text-orange-400 text-xs break-all">{selectedParam.name}</div>
                            </div>
                            <div>
                                <div class="text-xs text-zinc-500 mb-0.5">Type</div>
                                <span class="text-xs {typeColor(selectedParam.type)}">{selectedParam.type}</span>
                            </div>
                            <div>
                                <div class="text-xs text-zinc-500 mb-0.5">Version</div>
                                <div class="text-xs text-zinc-300">{selectedParam.version}</div>
                            </div>
                            <div>
                                <div class="text-xs text-zinc-500 mb-1">Value</div>
                                <div class="bg-zinc-800 rounded p-2 font-mono text-xs text-zinc-200 break-all whitespace-pre-wrap max-h-40 overflow-y-auto">
                                    {selectedParam.value}
                                </div>
                            </div>
                        </div>
                    {:else}
                        <div class="text-zinc-600 text-sm text-center py-4">
                            Click a parameter to view its value.
                        </div>
                    {/if}
                </div>
            </div>
        </div>
    {/if}
</div>
