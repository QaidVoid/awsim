<script lang="ts">
    import { onMount } from 'svelte';

    const BASE = 'http://localhost:4566';

    interface GraphqlApi {
        apiId: string;
        name: string;
        arn: string;
        authenticationType: string;
        schemaStatus: string;
        createdAt: string;
        uris: Record<string, string>;
    }

    interface ApiKey {
        id: string;
        description: string | null;
        expires: number;
    }

    interface DataSource {
        name: string;
        type: string;
        description: string | null;
    }

    let apis = $state<GraphqlApi[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let showCreateForm = $state(false);
    let newName = $state('');
    let newAuthType = $state('API_KEY');
    let creating = $state(false);
    let createError = $state<string | null>(null);

    let selectedApi = $state<GraphqlApi | null>(null);
    let apiKeys = $state<ApiKey[]>([]);
    let dataSources = $state<DataSource[]>([]);
    let detailLoading = $state(false);
    let confirmDelete = $state<string | null>(null);

    async function apiFetch(path: string, opts?: RequestInit) {
        const res = await fetch(`${BASE}${path}`, {
            ...opts,
            headers: {
                'Content-Type': 'application/json',
                'Authorization': 'AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/appsync/aws4_request, SignedHeaders=host, Signature=fake',
                ...(opts?.headers || {}),
            },
        });
        if (!res.ok) {
            const text = await res.text();
            throw new Error(text || `HTTP ${res.status}`);
        }
        const text = await res.text();
        return text ? JSON.parse(text) : {};
    }

    async function loadApis() {
        loading = true;
        error = null;
        try {
            const data = await apiFetch('/v1/apis');
            apis = data.graphqlApis ?? [];
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to load APIs';
        } finally {
            loading = false;
        }
    }

    async function handleCreate() {
        if (!newName.trim()) return;
        creating = true;
        createError = null;
        try {
            await apiFetch('/v1/apis', {
                method: 'POST',
                body: JSON.stringify({ name: newName.trim(), authenticationType: newAuthType }),
            });
            newName = '';
            showCreateForm = false;
            await loadApis();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed to create API';
        } finally {
            creating = false;
        }
    }

    async function handleDelete(apiId: string) {
        try {
            await apiFetch(`/v1/apis/${apiId}`, { method: 'DELETE' });
            confirmDelete = null;
            if (selectedApi?.apiId === apiId) selectedApi = null;
            await loadApis();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete API';
        }
    }

    async function selectApi(api: GraphqlApi) {
        selectedApi = api;
        detailLoading = true;
        apiKeys = [];
        dataSources = [];
        try {
            const [keysData, dsData] = await Promise.all([
                apiFetch(`/v1/apis/${api.apiId}/apikeys`),
                apiFetch(`/v1/apis/${api.apiId}/datasources`),
            ]);
            apiKeys = keysData.apiKeys ?? [];
            dataSources = dsData.dataSources ?? [];
        } catch {
            // silently fail
        } finally {
            detailLoading = false;
        }
    }

    function authColor(type: string): string {
        if (type === 'API_KEY') return 'bg-blue-900/40 text-blue-400';
        if (type === 'IAM') return 'bg-orange-900/40 text-orange-400';
        if (type === 'COGNITO_USER_POOLS') return 'bg-purple-900/40 text-purple-400';
        return 'bg-zinc-800 text-zinc-400';
    }

    function formatDate(iso: string): string {
        try { return new Date(iso).toLocaleString(); } catch { return iso; }
    }

    onMount(() => loadApis());
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">AppSync — GraphQL APIs</h1>
            <p class="text-zinc-500 mt-1">Managed GraphQL service for building APIs.</p>
        </div>
        <div class="flex items-center gap-3">
            <span class="text-sm text-zinc-500">{apis.length} API{apis.length !== 1 ? 's' : ''}</span>
            <button
                onclick={() => { showCreateForm = !showCreateForm; createError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create API
            </button>
        </div>
    </div>

    {#if showCreateForm}
        <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
            <h3 class="font-semibold mb-3">Create GraphQL API</h3>
            {#if createError}
                <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createError}</div>
            {/if}
            <div class="grid grid-cols-2 gap-3 mb-3">
                <div>
                    <label for="api-name" class="block text-xs text-zinc-400 mb-1">API Name</label>
                    <input
                        id="api-name"
                        type="text"
                        bind:value={newName}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        placeholder="my-graphql-api"
                    />
                </div>
                <div>
                    <label for="auth-type" class="block text-xs text-zinc-400 mb-1">Authentication Type</label>
                    <select
                        id="auth-type"
                        bind:value={newAuthType}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                    >
                        <option value="API_KEY">API_KEY</option>
                        <option value="IAM">IAM</option>
                        <option value="COGNITO_USER_POOLS">COGNITO_USER_POOLS</option>
                    </select>
                </div>
            </div>
            <div class="flex gap-2">
                <button
                    onclick={handleCreate}
                    disabled={creating || !newName.trim()}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                >
                    {creating ? 'Creating...' : 'Create'}
                </button>
                <button
                    onclick={() => { showCreateForm = false; createError = null; newName = ''; }}
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
    {:else if apis.length === 0 && !showCreateForm}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No GraphQL APIs yet.</p>
            <button onclick={() => showCreateForm = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                Create your first API
            </button>
        </div>
    {:else}
        <div class="flex gap-4">
            <!-- API list -->
            <div class="w-72 shrink-0">
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                    {#each apis as api}
                        <div class="border-b border-zinc-800/50 last:border-0 {selectedApi?.apiId === api.apiId ? 'bg-zinc-800' : 'hover:bg-zinc-800/40'} transition-colors">
                            <div class="px-4 py-3">
                                <button class="w-full text-left" onclick={() => selectApi(api)}>
                                    <div class="font-mono text-orange-400 text-sm truncate">{api.name}</div>
                                    <div class="text-xs text-zinc-500 mt-0.5 truncate">{api.apiId}</div>
                                    <span class="inline-block mt-1 px-1.5 py-0.5 rounded text-xs {authColor(api.authenticationType)}">{api.authenticationType}</span>
                                </button>
                                <div class="flex gap-1 mt-2">
                                    {#if confirmDelete === api.apiId}
                                        <button onclick={() => handleDelete(api.apiId)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                        <button onclick={() => confirmDelete = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                    {:else}
                                        <button onclick={(e) => { e.stopPropagation(); confirmDelete = api.apiId; }} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                    {/if}
                                </div>
                            </div>
                        </div>
                    {/each}
                </div>
            </div>

            <!-- Detail panel -->
            <div class="flex-1 min-w-0">
                {#if selectedApi}
                    <div class="space-y-4">
                        <!-- API info -->
                        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4">
                            <div class="text-sm font-semibold text-zinc-300 mb-3">{selectedApi.name}</div>
                            <div class="grid grid-cols-3 gap-4 text-sm">
                                <div>
                                    <div class="text-xs text-zinc-500 mb-1">API ID</div>
                                    <div class="font-mono text-orange-400 text-xs">{selectedApi.apiId}</div>
                                </div>
                                <div>
                                    <div class="text-xs text-zinc-500 mb-1">Auth Type</div>
                                    <span class="px-1.5 py-0.5 rounded text-xs {authColor(selectedApi.authenticationType)}">{selectedApi.authenticationType}</span>
                                </div>
                                <div>
                                    <div class="text-xs text-zinc-500 mb-1">Schema</div>
                                    <div class="text-xs text-zinc-300">{selectedApi.schemaStatus}</div>
                                </div>
                            </div>
                            {#if selectedApi.uris?.GRAPHQL}
                                <div class="mt-3">
                                    <div class="text-xs text-zinc-500 mb-1">GraphQL Endpoint</div>
                                    <div class="font-mono text-xs text-zinc-300 bg-zinc-800 rounded p-2 break-all">{selectedApi.uris.GRAPHQL}</div>
                                </div>
                            {/if}
                        </div>

                        {#if detailLoading}
                            <div class="text-zinc-500 text-sm">Loading details...</div>
                        {:else}
                            <!-- API Keys -->
                            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                                <div class="px-4 py-3 border-b border-zinc-800 flex items-center justify-between">
                                    <span class="text-sm font-medium text-zinc-300">API Keys</span>
                                    <span class="text-xs text-zinc-500">{apiKeys.length}</span>
                                </div>
                                {#if apiKeys.length === 0}
                                    <div class="px-4 py-3 text-zinc-500 text-sm">No API keys.</div>
                                {:else}
                                    <table class="w-full text-sm">
                                        <thead>
                                            <tr class="text-left text-zinc-500 border-b border-zinc-800">
                                                <th class="px-4 py-2 text-xs">ID</th>
                                                <th class="px-4 py-2 text-xs">Description</th>
                                                <th class="px-4 py-2 text-xs">Expires</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {#each apiKeys as key}
                                                <tr class="border-b border-zinc-800/50">
                                                    <td class="px-4 py-2 font-mono text-orange-400 text-xs">{key.id}</td>
                                                    <td class="px-4 py-2 text-zinc-400 text-xs">{key.description ?? '—'}</td>
                                                    <td class="px-4 py-2 text-zinc-400 text-xs">{new Date(key.expires * 1000).toLocaleDateString()}</td>
                                                </tr>
                                            {/each}
                                        </tbody>
                                    </table>
                                {/if}
                            </div>

                            <!-- Data Sources -->
                            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                                <div class="px-4 py-3 border-b border-zinc-800 flex items-center justify-between">
                                    <span class="text-sm font-medium text-zinc-300">Data Sources</span>
                                    <span class="text-xs text-zinc-500">{dataSources.length}</span>
                                </div>
                                {#if dataSources.length === 0}
                                    <div class="px-4 py-3 text-zinc-500 text-sm">No data sources.</div>
                                {:else}
                                    <table class="w-full text-sm">
                                        <thead>
                                            <tr class="text-left text-zinc-500 border-b border-zinc-800">
                                                <th class="px-4 py-2 text-xs">Name</th>
                                                <th class="px-4 py-2 text-xs">Type</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {#each dataSources as ds}
                                                <tr class="border-b border-zinc-800/50">
                                                    <td class="px-4 py-2 font-mono text-orange-400 text-xs">{ds.name}</td>
                                                    <td class="px-4 py-2 text-zinc-400 text-xs">{ds.type}</td>
                                                </tr>
                                            {/each}
                                        </tbody>
                                    </table>
                                {/if}
                            </div>
                        {/if}
                    </div>
                {:else}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-500 text-sm">
                        Select an API to view its details.
                    </div>
                {/if}
            </div>
        </div>
    {/if}
</div>
