<script lang="ts">
    import { onMount } from 'svelte';

    const BASE = 'http://localhost:4566';

    interface Location { LocationArn: string; LocationUri: string; }
    interface Task { TaskArn: string; Status: string; Name: string; }
    interface Execution { TaskExecutionArn: string; Status: string; }

    let activeTab = $state<'locations' | 'tasks' | 'executions'>('locations');
    let locations = $state<Location[]>([]);
    let tasks = $state<Task[]>([]);
    let executions = $state<Execution[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let showLocForm = $state(false);
    let newBucketArn = $state('');
    let newSubdir = $state('/');
    let creating = $state(false);
    let createError = $state<string | null>(null);

    async function apiFetch(target: string, body: unknown) {
        const res = await fetch(`${BASE}/`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/x-amz-json-1.1',
                'X-Amz-Target': `FmrsService.${target}`,
                'Authorization': 'AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/datasync/aws4_request, SignedHeaders=host, Signature=fake',
            },
            body: JSON.stringify(body ?? {}),
        });
        if (!res.ok) throw new Error(await res.text() || `HTTP ${res.status}`);
        const text = await res.text();
        return text ? JSON.parse(text) : {};
    }

    async function loadAll() {
        loading = true;
        error = null;
        try {
            const [l, t, e] = await Promise.all([
                apiFetch('ListLocations', {}),
                apiFetch('ListTasks', {}),
                apiFetch('ListTaskExecutions', {}),
            ]);
            locations = l.Locations ?? [];
            tasks = t.Tasks ?? [];
            executions = e.TaskExecutions ?? [];
        } catch (err) {
            error = err instanceof Error ? err.message : 'Failed';
        } finally {
            loading = false;
        }
    }

    async function createLocationS3() {
        if (!newBucketArn.trim()) return;
        creating = true;
        createError = null;
        try {
            await apiFetch('CreateLocationS3', {
                S3BucketArn: newBucketArn.trim(),
                Subdirectory: newSubdir.trim() || '/',
                S3Config: { BucketAccessRoleArn: 'arn:aws:iam::000000000000:role/datasync-s3' },
            });
            newBucketArn = '';
            showLocForm = false;
            await loadAll();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed';
        } finally {
            creating = false;
        }
    }

    async function deleteLocation(arn: string) {
        if (!confirm('Delete location?')) return;
        try {
            await apiFetch('DeleteLocation', { LocationArn: arn });
            await loadAll();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed';
        }
    }

    function statusColor(s: string): string {
        if (s === 'AVAILABLE' || s === 'SUCCESS') return 'bg-green-900/40 text-green-300';
        if (s === 'ERROR') return 'bg-red-900/40 text-red-300';
        return 'bg-zinc-800 text-zinc-400';
    }

    onMount(loadAll);
</script>

<div class="p-6">
    <div class="mb-6">
        <h1 class="text-2xl font-bold">DataSync</h1>
        <p class="text-zinc-500 mt-1">Locations, tasks, and executions.</p>
    </div>

    <div class="flex gap-1 mb-6 border-b border-zinc-800">
        <button onclick={() => activeTab = 'locations'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'locations' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Locations ({locations.length})</button>
        <button onclick={() => activeTab = 'tasks'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'tasks' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Tasks ({tasks.length})</button>
        <button onclick={() => activeTab = 'executions'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'executions' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Executions ({executions.length})</button>
    </div>

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if activeTab === 'locations'}
        <div class="flex items-center justify-between mb-4">
            <span class="text-sm text-zinc-400">{locations.length} location{locations.length !== 1 ? 's' : ''}</span>
            <button onclick={() => { showLocForm = !showLocForm; createError = null; }} class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">Create S3 Location</button>
        </div>
        {#if showLocForm}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                {#if createError}<p class="text-red-400 text-xs mb-2">{createError}</p>{/if}
                <div class="space-y-3">
                    <input type="text" bind:value={newBucketArn} placeholder="arn:aws:s3:::my-bucket" class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" />
                    <input type="text" bind:value={newSubdir} placeholder="/subdirectory" class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" />
                    <div class="flex gap-2">
                        <button onclick={createLocationS3} disabled={creating} class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm font-medium">{creating ? 'Creating...' : 'Create'}</button>
                        <button onclick={() => showLocForm = false} class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm">Cancel</button>
                    </div>
                </div>
            </div>
        {/if}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">ARN</th><th class="px-4 py-3 text-xs">URI</th><th class="px-4 py-3 text-xs"></th></tr></thead>
                <tbody>
                    {#each locations as l}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400 text-xs">{l.LocationArn}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs font-mono">{l.LocationUri}</td>
                            <td class="px-4 py-3 text-right"><button onclick={() => deleteLocation(l.LocationArn)} class="text-red-400 hover:text-red-300 text-xs">Delete</button></td>
                        </tr>
                    {/each}
                    {#if locations.length === 0}<tr><td colspan="3" class="px-4 py-8 text-center text-zinc-500">No locations.</td></tr>{/if}
                </tbody>
            </table>
        </div>
    {:else if activeTab === 'tasks'}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">ARN</th><th class="px-4 py-3 text-xs">Name</th><th class="px-4 py-3 text-xs">Status</th></tr></thead>
                <tbody>
                    {#each tasks as t}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400 text-xs">{t.TaskArn}</td>
                            <td class="px-4 py-3 text-zinc-200">{t.Name}</td>
                            <td class="px-4 py-3"><span class="px-1.5 py-0.5 rounded text-xs {statusColor(t.Status)}">{t.Status}</span></td>
                        </tr>
                    {/each}
                    {#if tasks.length === 0}<tr><td colspan="3" class="px-4 py-8 text-center text-zinc-500">No tasks.</td></tr>{/if}
                </tbody>
            </table>
        </div>
    {:else}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">Execution ARN</th><th class="px-4 py-3 text-xs">Status</th></tr></thead>
                <tbody>
                    {#each executions as e}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400 text-xs">{e.TaskExecutionArn}</td>
                            <td class="px-4 py-3"><span class="px-1.5 py-0.5 rounded text-xs {statusColor(e.Status)}">{e.Status}</span></td>
                        </tr>
                    {/each}
                    {#if executions.length === 0}<tr><td colspan="2" class="px-4 py-8 text-center text-zinc-500">No executions.</td></tr>{/if}
                </tbody>
            </table>
        </div>
    {/if}
</div>
