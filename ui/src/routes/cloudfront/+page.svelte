<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listDistributions,
        createDistribution,
        deleteDistribution,
        type CloudFrontDistribution,
    } from '$lib/aws';

    let distributions = $state<CloudFrontDistribution[]>([]);
    let loading = $state(false);
    let error = $state<string | null>(null);
    let showCreate = $state(false);
    let newOriginDomain = $state('');
    let newComment = $state('');
    let creating = $state(false);
    let createError = $state<string | null>(null);
    let confirmDelete = $state<string | null>(null);

    async function loadDistributions() {
        loading = true;
        error = null;
        try {
            const data = await listDistributions();
            distributions = data.distributions;
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to load distributions';
        } finally {
            loading = false;
        }
    }

    async function handleCreate() {
        if (!newOriginDomain.trim()) return;
        creating = true;
        createError = null;
        try {
            await createDistribution(newOriginDomain.trim(), newComment.trim());
            showCreate = false;
            newOriginDomain = '';
            newComment = '';
            await loadDistributions();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed to create distribution';
        } finally {
            creating = false;
        }
    }

    async function handleDelete(id: string) {
        try {
            await deleteDistribution(id);
            confirmDelete = null;
            await loadDistributions();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete distribution';
        }
    }

    onMount(() => loadDistributions());
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">CloudFront — Content Delivery Network</h1>
            <p class="text-zinc-500 mt-1">Manage CloudFront distributions and origin access controls.</p>
        </div>
        <button
            onclick={() => { showCreate = !showCreate; createError = null; }}
            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
        >
            Create Distribution
        </button>
    </div>

    {#if showCreate}
        <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
            <h3 class="font-semibold mb-3">Create Distribution</h3>
            {#if createError}
                <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createError}</div>
            {/if}
            <div class="grid grid-cols-2 gap-3 mb-3">
                <div>
                    <label for="cf-origin" class="block text-xs text-zinc-400 mb-1">Origin Domain Name</label>
                    <input
                        id="cf-origin"
                        type="text"
                        bind:value={newOriginDomain}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        placeholder="my-bucket.s3.amazonaws.com"
                    />
                </div>
                <div>
                    <label for="cf-comment" class="block text-xs text-zinc-400 mb-1">Comment (optional)</label>
                    <input
                        id="cf-comment"
                        type="text"
                        bind:value={newComment}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        placeholder="My distribution"
                    />
                </div>
            </div>
            <div class="flex gap-2">
                <button
                    onclick={handleCreate}
                    disabled={creating || !newOriginDomain.trim()}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                >
                    {creating ? 'Creating...' : 'Create'}
                </button>
                <button
                    onclick={() => { showCreate = false; createError = null; newOriginDomain = ''; newComment = ''; }}
                    class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                >
                    Cancel
                </button>
            </div>
        </div>
    {/if}

    {#if loading}
        <div class="text-zinc-500">Loading distributions...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if distributions.length === 0}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No distributions found.</p>
            <button onclick={() => showCreate = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                Create your first distribution
            </button>
        </div>
    {:else}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-zinc-800 text-left text-zinc-500">
                        <th class="px-4 py-3">ID</th>
                        <th class="px-4 py-3">Domain Name</th>
                        <th class="px-4 py-3">Status</th>
                        <th class="px-4 py-3">Comment</th>
                        <th class="px-4 py-3">Enabled</th>
                        <th class="px-4 py-3"></th>
                    </tr>
                </thead>
                <tbody>
                    {#each distributions as dist}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400 text-xs">{dist.id}</td>
                            <td class="px-4 py-3 font-mono text-zinc-300 text-xs">{dist.domainName}</td>
                            <td class="px-4 py-3">
                                <span class="px-2 py-0.5 rounded text-xs {dist.status === 'Deployed' ? 'bg-green-900/40 text-green-400' : 'bg-yellow-900/40 text-yellow-400'}">{dist.status}</span>
                            </td>
                            <td class="px-4 py-3 text-zinc-400 text-xs">{dist.comment || '-'}</td>
                            <td class="px-4 py-3">
                                <span class="px-2 py-0.5 rounded text-xs {dist.enabled ? 'bg-green-900/40 text-green-400' : 'bg-zinc-800 text-zinc-400'}">{dist.enabled ? 'Yes' : 'No'}</span>
                            </td>
                            <td class="px-4 py-3">
                                {#if confirmDelete === dist.id}
                                    <div class="flex items-center gap-1">
                                        <button onclick={() => handleDelete(dist.id)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                        <button onclick={() => confirmDelete = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                    </div>
                                {:else}
                                    <button onclick={() => confirmDelete = dist.id} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                {/if}
                            </td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    {/if}
</div>
