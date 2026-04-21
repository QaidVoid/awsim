<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listClusters, describeClusters, createCluster, deleteCluster,
        listTaskDefinitions,
        type EcsCluster,
    } from '$lib/aws';

    let activeTab = $state<'clusters' | 'taskDefinitions'>('clusters');

    // ---- Clusters ----
    let clusters = $state<EcsCluster[]>([]);
    let clustersLoading = $state(false);
    let clustersError = $state<string | null>(null);
    let showCreateCluster = $state(false);
    let newClusterName = $state('');
    let creatingCluster = $state(false);
    let createClusterError = $state<string | null>(null);
    let confirmDeleteCluster = $state<string | null>(null);

    // ---- Task Definitions ----
    let taskDefinitionArns = $state<string[]>([]);
    let tdLoading = $state(false);
    let tdError = $state<string | null>(null);

    async function loadClusters() {
        clustersLoading = true;
        clustersError = null;
        try {
            const { clusterArns } = await listClusters();
            if (clusterArns.length > 0) {
                const { clusters: described } = await describeClusters(clusterArns);
                clusters = described;
            } else {
                clusters = [];
            }
        } catch (e) {
            clustersError = e instanceof Error ? e.message : 'Failed to load clusters';
        } finally {
            clustersLoading = false;
        }
    }

    async function handleCreateCluster() {
        if (!newClusterName.trim()) return;
        creatingCluster = true;
        createClusterError = null;
        try {
            await createCluster(newClusterName.trim());
            newClusterName = '';
            showCreateCluster = false;
            await loadClusters();
        } catch (e) {
            createClusterError = e instanceof Error ? e.message : 'Failed to create cluster';
        } finally {
            creatingCluster = false;
        }
    }

    async function handleDeleteCluster(clusterArn: string) {
        try {
            await deleteCluster(clusterArn);
            confirmDeleteCluster = null;
            await loadClusters();
        } catch (e) {
            clustersError = e instanceof Error ? e.message : 'Failed to delete cluster';
        }
    }

    async function loadTaskDefinitions() {
        tdLoading = true;
        tdError = null;
        try {
            const { taskDefinitionArns: arns } = await listTaskDefinitions();
            taskDefinitionArns = arns;
        } catch (e) {
            tdError = e instanceof Error ? e.message : 'Failed to load task definitions';
        } finally {
            tdLoading = false;
        }
    }

    function switchTab(tab: 'clusters' | 'taskDefinitions') {
        activeTab = tab;
        if (tab === 'clusters' && clusters.length === 0 && !clustersLoading) loadClusters();
        if (tab === 'taskDefinitions' && taskDefinitionArns.length === 0 && !tdLoading) loadTaskDefinitions();
    }

    function clusterShortName(arn: string): string {
        return arn.split('/').pop() ?? arn;
    }

    function tdFamilyRevision(arn: string): string {
        // arn:aws:ecs:region:account:task-definition/family:revision
        const parts = arn.split('/');
        return parts.pop() ?? arn;
    }

    onMount(() => loadClusters());
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">ECS — Elastic Container Service</h1>
            <p class="text-zinc-500 mt-1">Run and manage Docker containers in clusters.</p>
        </div>
        {#if activeTab === 'clusters'}
            <button
                onclick={() => { showCreateCluster = !showCreateCluster; createClusterError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Cluster
            </button>
        {/if}
    </div>

    <!-- Tab nav -->
    <div class="flex gap-1 mb-4 border-b border-zinc-800">
        {#each [['clusters', 'Clusters'], ['taskDefinitions', 'Task Definitions']] as [tab, label]}
            <button
                onclick={() => switchTab(tab as 'clusters' | 'taskDefinitions')}
                class="px-4 py-2 text-sm font-medium border-b-2 transition-colors {activeTab === tab ? 'border-orange-400 text-orange-400' : 'border-transparent text-zinc-500 hover:text-zinc-300'}"
            >
                {label}
            </button>
        {/each}
    </div>

    <!-- Clusters tab -->
    {#if activeTab === 'clusters'}
        {#if showCreateCluster}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create Cluster</h3>
                {#if createClusterError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createClusterError}</div>
                {/if}
                <label for="cluster-name" class="block text-xs text-zinc-400 mb-1">Cluster Name</label>
                <input
                    id="cluster-name"
                    type="text"
                    bind:value={newClusterName}
                    onkeydown={(e) => e.key === 'Enter' && handleCreateCluster()}
                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                    placeholder="my-cluster"
                />
                <div class="flex gap-2 mt-3">
                    <button
                        onclick={handleCreateCluster}
                        disabled={creatingCluster || !newClusterName.trim()}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                    >
                        {creatingCluster ? 'Creating...' : 'Create'}
                    </button>
                    <button
                        onclick={() => { showCreateCluster = false; createClusterError = null; newClusterName = ''; }}
                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        {/if}

        {#if clustersLoading}
            <div class="text-zinc-500">Loading clusters...</div>
        {:else if clustersError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{clustersError}</div>
        {:else if clusters.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No ECS clusters yet.</p>
                <button onclick={() => showCreateCluster = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                    Create your first cluster
                </button>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Name</th>
                            <th class="px-4 py-3">ARN</th>
                            <th class="px-4 py-3">Status</th>
                            <th class="px-4 py-3"></th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each clusters as cluster}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400">{cluster.clusterName}</td>
                                <td class="px-4 py-3 text-zinc-400 text-xs font-mono truncate max-w-xs">{cluster.clusterArn}</td>
                                <td class="px-4 py-3">
                                    <span class="px-2 py-0.5 rounded text-xs {cluster.status === 'ACTIVE' ? 'bg-green-900/40 text-green-400' : 'bg-zinc-800 text-zinc-400'}">{cluster.status}</span>
                                </td>
                                <td class="px-4 py-3">
                                    {#if confirmDeleteCluster === cluster.clusterArn}
                                        <div class="flex items-center gap-1">
                                            <button onclick={() => handleDeleteCluster(cluster.clusterArn)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteCluster = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button onclick={() => confirmDeleteCluster = cluster.clusterArn} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                    {/if}
                                </td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}

    <!-- Task Definitions tab -->
    {#if activeTab === 'taskDefinitions'}
        {#if tdLoading}
            <div class="text-zinc-500">Loading task definitions...</div>
        {:else if tdError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{tdError}</div>
        {:else if taskDefinitionArns.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No task definitions found.</p>
                <code class="block mt-3 text-xs text-orange-400 font-mono">
                    aws --endpoint-url http://localhost:4566 ecs register-task-definition --family my-task --container-definitions '[...]'
                </code>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Family:Revision</th>
                            <th class="px-4 py-3">ARN</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each taskDefinitionArns as arn}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400">{tdFamilyRevision(arn)}</td>
                                <td class="px-4 py-3 text-zinc-400 text-xs font-mono">{arn}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}
</div>
