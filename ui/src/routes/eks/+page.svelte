<script lang="ts">
    import { onMount } from 'svelte';

    const BASE = 'http://localhost:4566';

    interface Cluster { name: string; arn: string; status: string; version: string; endpoint?: string; }

    let activeTab = $state<'clusters' | 'nodegroups' | 'fargate'>('clusters');
    let clusters = $state<Cluster[]>([]);
    let nodegroupsByCluster = $state<Record<string, string[]>>({});
    let fargateByCluster = $state<Record<string, string[]>>({});
    let loading = $state(true);
    let error = $state<string | null>(null);

    let showCreate = $state(false);
    let newName = $state('');
    let newVersion = $state('1.29');
    let newRoleArn = $state('arn:aws:iam::000000000000:role/eks-cluster');
    let creating = $state(false);
    let createError = $state<string | null>(null);

    async function apiFetch(path: string, opts: RequestInit = {}) {
        const res = await fetch(`${BASE}${path}`, {
            headers: {
                'Content-Type': 'application/json',
                'Authorization': 'AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/eks/aws4_request, SignedHeaders=host, Signature=fake',
            },
            ...opts,
        });
        if (!res.ok) throw new Error(await res.text() || `HTTP ${res.status}`);
        const text = await res.text();
        return text ? JSON.parse(text) : {};
    }

    async function loadClusters() {
        loading = true;
        error = null;
        try {
            const data = await apiFetch('/clusters');
            const names: string[] = data.clusters ?? [];
            const described: Cluster[] = [];
            for (const n of names) {
                try {
                    const d = await apiFetch(`/clusters/${encodeURIComponent(n)}`);
                    if (d.cluster) described.push(d.cluster);
                } catch { /* skip */ }
            }
            clusters = described;

            const ng: Record<string, string[]> = {};
            const fg: Record<string, string[]> = {};
            for (const n of names) {
                try {
                    const r = await apiFetch(`/clusters/${encodeURIComponent(n)}/node-groups`);
                    ng[n] = r.nodegroups ?? [];
                } catch { ng[n] = []; }
                try {
                    const r = await apiFetch(`/clusters/${encodeURIComponent(n)}/fargate-profiles`);
                    fg[n] = r.fargateProfileNames ?? r.fargateProfiles ?? [];
                } catch { fg[n] = []; }
            }
            nodegroupsByCluster = ng;
            fargateByCluster = fg;
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed';
        } finally {
            loading = false;
        }
    }

    async function createCluster() {
        if (!newName.trim()) return;
        creating = true;
        createError = null;
        try {
            await apiFetch('/clusters', {
                method: 'POST',
                body: JSON.stringify({
                    name: newName.trim(),
                    version: newVersion.trim(),
                    roleArn: newRoleArn.trim(),
                    resourcesVpcConfig: { subnetIds: ['subnet-1', 'subnet-2'] },
                }),
            });
            newName = '';
            showCreate = false;
            await loadClusters();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed';
        } finally {
            creating = false;
        }
    }

    async function deleteCluster(name: string) {
        if (!confirm(`Delete cluster ${name}?`)) return;
        try {
            await apiFetch(`/clusters/${encodeURIComponent(name)}`, { method: 'DELETE' });
            await loadClusters();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed';
        }
    }

    function statusColor(s: string): string {
        if (s === 'ACTIVE') return 'bg-green-900/40 text-green-300';
        if (s === 'FAILED') return 'bg-red-900/40 text-red-300';
        return 'bg-zinc-800 text-zinc-400';
    }

    onMount(loadClusters);
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">EKS</h1>
            <p class="text-zinc-500 mt-1">Kubernetes clusters, nodegroups, and Fargate profiles.</p>
        </div>
        {#if activeTab === 'clusters'}
            <button onclick={() => { showCreate = !showCreate; createError = null; }} class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">Create Cluster</button>
        {/if}
    </div>

    <div class="flex gap-1 mb-6 border-b border-zinc-800">
        <button onclick={() => activeTab = 'clusters'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'clusters' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Clusters ({clusters.length})</button>
        <button onclick={() => activeTab = 'nodegroups'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'nodegroups' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Nodegroups</button>
        <button onclick={() => activeTab = 'fargate'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'fargate' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Fargate Profiles</button>
    </div>

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if activeTab === 'clusters'}
        {#if showCreate}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create Cluster</h3>
                {#if createError}<p class="text-red-400 text-xs mb-2">{createError}</p>{/if}
                <div class="space-y-3">
                    <input type="text" bind:value={newName} placeholder="cluster name" class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" />
                    <input type="text" bind:value={newVersion} placeholder="k8s version" class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" />
                    <input type="text" bind:value={newRoleArn} placeholder="role arn" class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" />
                    <div class="flex gap-2">
                        <button onclick={createCluster} disabled={creating} class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm font-medium">{creating ? 'Creating...' : 'Create'}</button>
                        <button onclick={() => showCreate = false} class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm">Cancel</button>
                    </div>
                </div>
            </div>
        {/if}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">Name</th><th class="px-4 py-3 text-xs">Version</th><th class="px-4 py-3 text-xs">Status</th><th class="px-4 py-3 text-xs">ARN</th><th class="px-4 py-3 text-xs"></th></tr></thead>
                <tbody>
                    {#each clusters as c}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 text-zinc-200">{c.name}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs">{c.version}</td>
                            <td class="px-4 py-3"><span class="px-1.5 py-0.5 rounded text-xs {statusColor(c.status)}">{c.status}</span></td>
                            <td class="px-4 py-3 font-mono text-orange-400 text-xs truncate max-w-xs">{c.arn}</td>
                            <td class="px-4 py-3 text-right"><button onclick={() => deleteCluster(c.name)} class="text-red-400 hover:text-red-300 text-xs">Delete</button></td>
                        </tr>
                    {/each}
                    {#if clusters.length === 0}<tr><td colspan="5" class="px-4 py-8 text-center text-zinc-500">No clusters.</td></tr>{/if}
                </tbody>
            </table>
        </div>
    {:else if activeTab === 'nodegroups'}
        {#each clusters as c}
            <div class="mb-4">
                <h3 class="text-sm font-semibold text-zinc-300 mb-2">{c.name}</h3>
                {#if (nodegroupsByCluster[c.name] ?? []).length === 0}
                    <div class="text-xs text-zinc-500 pl-4">No nodegroups.</div>
                {:else}
                    <ul class="space-y-1 pl-4">
                        {#each nodegroupsByCluster[c.name] as n}
                            <li class="font-mono text-xs text-orange-400">{n}</li>
                        {/each}
                    </ul>
                {/if}
            </div>
        {/each}
        {#if clusters.length === 0}<div class="text-zinc-500">No clusters.</div>{/if}
    {:else}
        {#each clusters as c}
            <div class="mb-4">
                <h3 class="text-sm font-semibold text-zinc-300 mb-2">{c.name}</h3>
                {#if (fargateByCluster[c.name] ?? []).length === 0}
                    <div class="text-xs text-zinc-500 pl-4">No Fargate profiles.</div>
                {:else}
                    <ul class="space-y-1 pl-4">
                        {#each fargateByCluster[c.name] as f}
                            <li class="font-mono text-xs text-orange-400">{f}</li>
                        {/each}
                    </ul>
                {/if}
            </div>
        {/each}
        {#if clusters.length === 0}<div class="text-zinc-500">No clusters.</div>{/if}
    {/if}
</div>
