<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listRepositories, createRepository, deleteRepository,
        type EcrRepository,
    } from '$lib/aws';

    let repositories = $state<EcrRepository[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let showCreateForm = $state(false);
    let newRepoName = $state('');
    let creating = $state(false);
    let createError = $state<string | null>(null);
    let confirmDeleteRepo = $state<string | null>(null);

    async function loadRepositories() {
        loading = true;
        error = null;
        try {
            const data = await listRepositories();
            repositories = data.repositories;
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to load repositories';
        } finally {
            loading = false;
        }
    }

    async function handleCreateRepository() {
        if (!newRepoName.trim()) return;
        creating = true;
        createError = null;
        try {
            await createRepository(newRepoName.trim());
            newRepoName = '';
            showCreateForm = false;
            await loadRepositories();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed to create repository';
        } finally {
            creating = false;
        }
    }

    async function handleDeleteRepository(name: string) {
        try {
            await deleteRepository(name);
            confirmDeleteRepo = null;
            await loadRepositories();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete repository';
        }
    }

    function formatDate(iso: string): string {
        try { return new Date(iso).toLocaleString(); } catch { return iso; }
    }

    onMount(() => loadRepositories());
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">ECR — Elastic Container Registry</h1>
            <p class="text-zinc-500 mt-1">Store, manage, and deploy container images.</p>
        </div>
        <div class="flex items-center gap-3">
            <span class="text-sm text-zinc-500">{repositories.length} repositor{repositories.length !== 1 ? 'ies' : 'y'}</span>
            <button
                onclick={() => { showCreateForm = !showCreateForm; createError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Repository
            </button>
        </div>
    </div>

    {#if showCreateForm}
        <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
            <h3 class="font-semibold mb-3">Create Repository</h3>
            {#if createError}
                <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createError}</div>
            {/if}
            <label for="repo-name" class="block text-xs text-zinc-400 mb-1">Repository Name</label>
            <input
                id="repo-name"
                type="text"
                bind:value={newRepoName}
                onkeydown={(e) => e.key === 'Enter' && handleCreateRepository()}
                class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                placeholder="my-repository"
            />
            <div class="flex gap-2 mt-3">
                <button
                    onclick={handleCreateRepository}
                    disabled={creating || !newRepoName.trim()}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                >
                    {creating ? 'Creating...' : 'Create'}
                </button>
                <button
                    onclick={() => { showCreateForm = false; createError = null; newRepoName = ''; }}
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
    {:else if repositories.length === 0 && !showCreateForm}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No ECR repositories yet.</p>
            <button
                onclick={() => showCreateForm = true}
                class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium"
            >
                Create your first repository
            </button>
        </div>
    {:else}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-zinc-800 text-left text-zinc-500">
                        <th class="px-4 py-3">Name</th>
                        <th class="px-4 py-3">URI</th>
                        <th class="px-4 py-3">ARN</th>
                        <th class="px-4 py-3">Created</th>
                        <th class="px-4 py-3"></th>
                    </tr>
                </thead>
                <tbody>
                    {#each repositories as repo}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400">{repo.repositoryName}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs font-mono truncate max-w-xs">{repo.repositoryUri}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs font-mono truncate max-w-xs">{repo.repositoryArn}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs">{formatDate(repo.createdAt)}</td>
                            <td class="px-4 py-3">
                                {#if confirmDeleteRepo === repo.repositoryName}
                                    <div class="flex items-center gap-1">
                                        <button onclick={() => handleDeleteRepository(repo.repositoryName)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                        <button onclick={() => confirmDeleteRepo = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                    </div>
                                {:else}
                                    <button onclick={() => confirmDeleteRepo = repo.repositoryName} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                {/if}
                            </td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    {/if}
</div>
