<script lang="ts">
    import { onMount } from 'svelte';
    import { listBuckets, createBucket, deleteBucket, listObjects, type S3Bucket, type S3Object, type S3CommonPrefix } from '$lib/aws';

    let buckets = $state<S3Bucket[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let showCreateForm = $state(false);
    let newBucketName = $state('');
    let creating = $state(false);
    let createError = $state<string | null>(null);

    let selectedBucket = $state<string | null>(null);
    let prefix = $state('');
    let objects = $state<S3Object[]>([]);
    let commonPrefixes = $state<S3CommonPrefix[]>([]);
    let objectsLoading = $state(false);
    let objectsError = $state<string | null>(null);

    let confirmDeleteBucket = $state<string | null>(null);
    let confirmDeleteObject = $state<string | null>(null);

    let bucketObjectCounts = $state<Record<string, number>>({});

    async function loadBuckets() {
        loading = true;
        error = null;
        try {
            const data = await listBuckets();
            buckets = data.buckets;
            // Load object counts in background
            for (const bucket of data.buckets) {
                listObjects(bucket.name, '', '/').then((res) => {
                    bucketObjectCounts = { ...bucketObjectCounts, [bucket.name]: res.objects.length + res.commonPrefixes.length };
                }).catch(() => {});
            }
        } catch {
            error = 'Could not connect to AWSim. Is it running on port 4566?';
        } finally {
            loading = false;
        }
    }

    async function handleCreateBucket() {
        if (!newBucketName.trim()) return;
        creating = true;
        createError = null;
        try {
            await createBucket(newBucketName.trim());
            newBucketName = '';
            showCreateForm = false;
            await loadBuckets();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed to create bucket';
        } finally {
            creating = false;
        }
    }

    async function handleDeleteBucket(name: string) {
        try {
            await deleteBucket(name);
            confirmDeleteBucket = null;
            if (selectedBucket === name) {
                selectedBucket = null;
                prefix = '';
                objects = [];
                commonPrefixes = [];
            }
            await loadBuckets();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete bucket';
        }
    }

    async function openBucket(name: string) {
        selectedBucket = name;
        prefix = '';
        await loadObjects(name, '');
    }

    async function loadObjects(bucket: string, pfx: string) {
        objectsLoading = true;
        objectsError = null;
        try {
            const res = await listObjects(bucket, pfx, '/');
            objects = res.objects;
            commonPrefixes = res.commonPrefixes;
        } catch (e) {
            objectsError = e instanceof Error ? e.message : 'Failed to list objects';
        } finally {
            objectsLoading = false;
        }
    }

    async function navigatePrefix(newPrefix: string) {
        prefix = newPrefix;
        if (selectedBucket) {
            await loadObjects(selectedBucket, newPrefix);
        }
    }

    async function handleDeleteObject(key: string) {
        if (!selectedBucket) return;
        try {
            const { deleteObject } = await import('$lib/aws');
            await deleteObject(selectedBucket, key);
            confirmDeleteObject = null;
            await loadObjects(selectedBucket, prefix);
        } catch (e) {
            objectsError = e instanceof Error ? e.message : 'Failed to delete object';
        }
    }

    let breadcrumbs = $derived(() => {
        if (!selectedBucket) return [];
        const parts = prefix.split('/').filter(Boolean);
        const crumbs = [{ label: selectedBucket, pfx: '' }];
        let built = '';
        for (const part of parts) {
            built += part + '/';
            crumbs.push({ label: part, pfx: built });
        }
        return crumbs;
    });

    function formatBytes(bytes: number): string {
        if (bytes === 0) return '0 B';
        const k = 1024;
        const sizes = ['B', 'KB', 'MB', 'GB'];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        return `${(bytes / Math.pow(k, i)).toFixed(1)} ${sizes[i]}`;
    }

    function formatDate(iso: string): string {
        try {
            return new Date(iso).toLocaleString();
        } catch {
            return iso;
        }
    }

    onMount(loadBuckets);
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">S3 — Buckets</h1>
            <p class="text-zinc-500 mt-1">Simple Storage Service. Store and retrieve objects in buckets.</p>
        </div>
        <div class="flex items-center gap-3">
            <span class="text-sm text-zinc-500">{buckets.length} bucket{buckets.length !== 1 ? 's' : ''}</span>
            <button
                onclick={() => { showCreateForm = !showCreateForm; createError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Bucket
            </button>
        </div>
    </div>

    {#if showCreateForm}
        <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
            <h3 class="font-semibold mb-3">Create Bucket</h3>
            {#if createError}
                <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createError}</div>
            {/if}
            <input
                type="text"
                bind:value={newBucketName}
                onkeydown={(e) => e.key === 'Enter' && handleCreateBucket()}
                class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                placeholder="my-bucket-name"
            />
            <div class="flex gap-2 mt-3">
                <button
                    onclick={handleCreateBucket}
                    disabled={creating || !newBucketName.trim()}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                >
                    {creating ? 'Creating...' : 'Create'}
                </button>
                <button
                    onclick={() => { showCreateForm = false; createError = null; newBucketName = ''; }}
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
    {:else if buckets.length === 0 && !showCreateForm}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No buckets yet.</p>
            <button
                onclick={() => showCreateForm = true}
                class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium"
            >
                Create your first bucket
            </button>
        </div>
    {:else}
        <div class="flex gap-4">
            <!-- Bucket list -->
            <div class="w-72 shrink-0">
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                    {#each buckets as bucket}
                        <div
                            class="border-b border-zinc-800/50 last:border-0 {selectedBucket === bucket.name ? 'bg-zinc-800' : 'hover:bg-zinc-800/40'} cursor-pointer transition-colors"
                        >
                            <div class="px-4 py-3 flex items-start justify-between gap-2">
                                <button
                                    class="flex-1 text-left min-w-0"
                                    onclick={() => openBucket(bucket.name)}
                                >
                                    <div class="font-mono text-orange-400 text-sm truncate">{bucket.name}</div>
                                    <div class="text-xs text-zinc-500 mt-0.5">{formatDate(bucket.creationDate)}</div>
                                    {#if bucketObjectCounts[bucket.name] !== undefined}
                                        <div class="text-xs text-zinc-600 mt-0.5">{bucketObjectCounts[bucket.name]} item{bucketObjectCounts[bucket.name] !== 1 ? 's' : ''}</div>
                                    {/if}
                                </button>
                                <button
                                    onclick={(e) => { e.stopPropagation(); confirmDeleteBucket = bucket.name; }}
                                    class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs shrink-0 transition-colors"
                                >
                                    Delete
                                </button>
                            </div>
                            {#if confirmDeleteBucket === bucket.name}
                                <div class="px-4 pb-3 bg-red-900/10 border-t border-red-900/30">
                                    <p class="text-xs text-red-400 mb-2">Delete "{bucket.name}"? This cannot be undone.</p>
                                    <div class="flex gap-2">
                                        <button
                                            onclick={() => handleDeleteBucket(bucket.name)}
                                            class="px-2 py-1 bg-red-700 hover:bg-red-600 rounded text-xs font-medium"
                                        >
                                            Confirm Delete
                                        </button>
                                        <button
                                            onclick={() => confirmDeleteBucket = null}
                                            class="px-2 py-1 bg-zinc-700 hover:bg-zinc-600 rounded text-xs"
                                        >
                                            Cancel
                                        </button>
                                    </div>
                                </div>
                            {/if}
                        </div>
                    {/each}
                </div>
            </div>

            <!-- Object browser -->
            <div class="flex-1 min-w-0">
                {#if selectedBucket}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                        <!-- Breadcrumb -->
                        <div class="px-4 py-3 border-b border-zinc-800 flex items-center gap-1 text-sm flex-wrap">
                            {#each breadcrumbs() as crumb, i}
                                {#if i > 0}
                                    <span class="text-zinc-600">/</span>
                                {/if}
                                <button
                                    onclick={() => navigatePrefix(crumb.pfx)}
                                    class="{i === breadcrumbs().length - 1 ? 'text-zinc-300' : 'text-orange-400 hover:text-orange-300'} font-mono transition-colors"
                                >
                                    {crumb.label}
                                </button>
                            {/each}
                        </div>

                        {#if objectsLoading}
                            <div class="p-4 text-zinc-500 text-sm">Loading objects...</div>
                        {:else if objectsError}
                            <div class="p-4 text-red-400 text-sm">{objectsError}</div>
                        {:else if objects.length === 0 && commonPrefixes.length === 0}
                            <div class="p-8 text-center text-zinc-500 text-sm">
                                {prefix ? 'No objects in this folder.' : 'Bucket is empty.'}
                            </div>
                        {:else}
                            <table class="w-full text-sm">
                                <thead>
                                    <tr class="border-b border-zinc-800 text-left text-zinc-500">
                                        <th class="px-4 py-2">Name</th>
                                        <th class="px-4 py-2">Size</th>
                                        <th class="px-4 py-2">Last Modified</th>
                                        <th class="px-4 py-2"></th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {#each commonPrefixes as cp}
                                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                            <td class="px-4 py-2">
                                                <button
                                                    onclick={() => navigatePrefix(cp.prefix)}
                                                    class="font-mono text-orange-400 hover:text-orange-300 flex items-center gap-1 transition-colors"
                                                >
                                                    <span class="text-zinc-500 text-xs">folder</span>
                                                    {cp.prefix.slice(prefix.length)}
                                                </button>
                                            </td>
                                            <td class="px-4 py-2 text-zinc-500">—</td>
                                            <td class="px-4 py-2 text-zinc-500">—</td>
                                            <td class="px-4 py-2"></td>
                                        </tr>
                                    {/each}
                                    {#each objects as obj}
                                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                            <td class="px-4 py-2 font-mono text-sm text-zinc-200 break-all">{obj.key.slice(prefix.length)}</td>
                                            <td class="px-4 py-2 text-zinc-400 whitespace-nowrap">{formatBytes(obj.size)}</td>
                                            <td class="px-4 py-2 text-zinc-400 whitespace-nowrap text-xs">{formatDate(obj.lastModified)}</td>
                                            <td class="px-4 py-2">
                                                {#if confirmDeleteObject === obj.key}
                                                    <div class="flex items-center gap-1">
                                                        <button
                                                            onclick={() => handleDeleteObject(obj.key)}
                                                            class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs"
                                                        >
                                                            Confirm
                                                        </button>
                                                        <button
                                                            onclick={() => confirmDeleteObject = null}
                                                            class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs"
                                                        >
                                                            Cancel
                                                        </button>
                                                    </div>
                                                {:else}
                                                    <button
                                                        onclick={() => confirmDeleteObject = obj.key}
                                                        class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors"
                                                    >
                                                        Delete
                                                    </button>
                                                {/if}
                                            </td>
                                        </tr>
                                    {/each}
                                </tbody>
                            </table>
                        {/if}
                    </div>
                {:else}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-500 text-sm">
                        Select a bucket to browse objects.
                    </div>
                {/if}
            </div>
        </div>
    {/if}
</div>
