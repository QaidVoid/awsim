<script lang="ts">
    import { onMount } from 'svelte';

    const BASE = 'http://localhost:4566';

    interface Trail {
        Name: string;
        TrailARN: string;
        S3BucketName: string;
        HomeRegion: string;
        IsMultiRegionTrail?: boolean;
    }

    let trails = $state<Trail[]>([]);
    let statuses = $state<Record<string, boolean>>({});
    let loading = $state(true);
    let error = $state<string | null>(null);

    let showCreate = $state(false);
    let newName = $state('');
    let newBucket = $state('');
    let creating = $state(false);
    let createError = $state<string | null>(null);

    async function apiFetch(target: string, body: unknown) {
        const res = await fetch(`${BASE}/`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/x-amz-json-1.1',
                'X-Amz-Target': `CloudTrail_20131101.${target}`,
                'Authorization': 'AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/cloudtrail/aws4_request, SignedHeaders=host, Signature=fake',
            },
            body: JSON.stringify(body ?? {}),
        });
        if (!res.ok) throw new Error(await res.text() || `HTTP ${res.status}`);
        const text = await res.text();
        return text ? JSON.parse(text) : {};
    }

    async function loadTrails() {
        loading = true;
        error = null;
        try {
            const data = await apiFetch('DescribeTrails', {});
            trails = data.trailList ?? [];
            const map: Record<string, boolean> = {};
            for (const t of trails) {
                try {
                    const s = await apiFetch('GetTrailStatus', { Name: t.Name });
                    map[t.Name] = !!s.IsLogging;
                } catch {
                    map[t.Name] = false;
                }
            }
            statuses = map;
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed';
        } finally {
            loading = false;
        }
    }

    async function createTrail() {
        if (!newName.trim() || !newBucket.trim()) return;
        creating = true;
        createError = null;
        try {
            await apiFetch('CreateTrail', { Name: newName.trim(), S3BucketName: newBucket.trim() });
            newName = '';
            newBucket = '';
            showCreate = false;
            await loadTrails();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed';
        } finally {
            creating = false;
        }
    }

    async function toggleLogging(name: string, isLogging: boolean) {
        try {
            await apiFetch(isLogging ? 'StopLogging' : 'StartLogging', { Name: name });
            await loadTrails();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed';
        }
    }

    async function deleteTrail(name: string) {
        if (!confirm(`Delete trail ${name}?`)) return;
        try {
            await apiFetch('DeleteTrail', { Name: name });
            await loadTrails();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed';
        }
    }

    onMount(loadTrails);
</script>

<div class="p-6 max-w-6xl mx-auto">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">CloudTrail</h1>
            <p class="text-zinc-500 mt-1">Audit trails for AWS API activity.</p>
        </div>
        <button onclick={() => { showCreate = !showCreate; createError = null; }} class="px-4 py-2 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">Create Trail</button>
    </div>

    {#if showCreate}
        <div class="mb-6 p-4 bg-zinc-900 border border-zinc-700 rounded-lg">
            <h2 class="text-sm font-semibold mb-3">Create Trail</h2>
            {#if createError}<p class="text-red-400 text-xs mb-2">{createError}</p>{/if}
            <div class="space-y-3">
                <input type="text" bind:value={newName} placeholder="Trail name" class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm focus:outline-none focus:border-orange-500" />
                <input type="text" bind:value={newBucket} placeholder="S3 Bucket Name" class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm focus:outline-none focus:border-orange-500" />
                <div class="flex gap-2">
                    <button onclick={createTrail} disabled={creating} class="px-4 py-2 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm">{creating ? 'Creating...' : 'Create'}</button>
                    <button onclick={() => showCreate = false} class="px-4 py-2 bg-zinc-700 hover:bg-zinc-600 rounded text-sm">Cancel</button>
                </div>
            </div>
        </div>
    {/if}

    {#if loading}
        <p class="text-zinc-500 text-sm">Loading...</p>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if trails.length === 0}
        <div class="text-center py-16 text-zinc-500">No trails.</div>
    {:else}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-zinc-800 text-left text-zinc-500">
                        <th class="px-4 py-3 text-xs">Name</th>
                        <th class="px-4 py-3 text-xs">S3 Bucket</th>
                        <th class="px-4 py-3 text-xs">Home Region</th>
                        <th class="px-4 py-3 text-xs">Logging</th>
                        <th class="px-4 py-3 text-xs text-right"></th>
                    </tr>
                </thead>
                <tbody>
                    {#each trails as t}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 text-zinc-200">{t.Name}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs font-mono">{t.S3BucketName}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs">{t.HomeRegion}</td>
                            <td class="px-4 py-3">
                                {#if statuses[t.Name]}
                                    <span class="px-1.5 py-0.5 rounded text-xs bg-green-900/40 text-green-300">Logging</span>
                                {:else}
                                    <span class="px-1.5 py-0.5 rounded text-xs bg-zinc-800 text-zinc-400">Stopped</span>
                                {/if}
                            </td>
                            <td class="px-4 py-3 text-right space-x-2">
                                <button onclick={() => toggleLogging(t.Name, !!statuses[t.Name])} class="px-2 py-1 text-xs bg-zinc-700 hover:bg-zinc-600 rounded">
                                    {statuses[t.Name] ? 'Stop' : 'Start'}
                                </button>
                                <button onclick={() => deleteTrail(t.Name)} class="px-2 py-1 text-xs text-red-400 hover:text-red-300">Delete</button>
                            </td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    {/if}
</div>
