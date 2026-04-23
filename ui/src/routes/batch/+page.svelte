<script lang="ts">
    import { onMount } from 'svelte';

    const BASE = 'http://localhost:4566';

    interface ComputeEnv { computeEnvironmentName: string; computeEnvironmentArn: string; type: string; state: string; status: string; }
    interface JobQueue { jobQueueName: string; jobQueueArn: string; priority: number; state: string; status: string; }
    interface JobSummary { jobId: string; jobName: string; status: string; createdAt?: number; }

    let activeTab = $state<'envs' | 'queues' | 'jobs'>('envs');

    let envs = $state<ComputeEnv[]>([]);
    let queues = $state<JobQueue[]>([]);
    let jobs = $state<JobSummary[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let showSubmit = $state(false);
    let jobName = $state('');
    let jobQueue = $state('');
    let jobDefinition = $state('');
    let submitting = $state(false);
    let submitError = $state<string | null>(null);

    async function apiFetch(path: string, body: unknown) {
        const res = await fetch(`${BASE}${path}`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'Authorization': 'AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/batch/aws4_request, SignedHeaders=host, Signature=fake',
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
            const [e, q, j] = await Promise.all([
                apiFetch('/v1/describecomputeenvironments', {}),
                apiFetch('/v1/describejobqueues', {}),
                apiFetch('/v1/listjobs', {}),
            ]);
            envs = e.computeEnvironments ?? [];
            queues = q.jobQueues ?? [];
            jobs = j.jobSummaryList ?? [];
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to load';
        } finally {
            loading = false;
        }
    }

    async function submitJob() {
        if (!jobName.trim() || !jobQueue.trim() || !jobDefinition.trim()) return;
        submitting = true;
        submitError = null;
        try {
            await apiFetch('/v1/submitjob', {
                jobName: jobName.trim(),
                jobQueue: jobQueue.trim(),
                jobDefinition: jobDefinition.trim(),
            });
            jobName = '';
            showSubmit = false;
            await loadAll();
        } catch (e) {
            submitError = e instanceof Error ? e.message : 'Submit failed';
        } finally {
            submitting = false;
        }
    }

    async function terminateJob(id: string) {
        if (!confirm(`Terminate job ${id}?`)) return;
        try {
            await apiFetch('/v1/terminatejob', { jobId: id, reason: 'User terminated' });
            await loadAll();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed';
        }
    }

    function statusColor(s: string): string {
        if (s === 'SUCCEEDED' || s === 'VALID' || s === 'ENABLED') return 'bg-green-900/40 text-green-300';
        if (s === 'FAILED') return 'bg-red-900/40 text-red-300';
        if (s === 'RUNNING' || s === 'RUNNABLE') return 'bg-blue-900/40 text-blue-300';
        return 'bg-zinc-800 text-zinc-400';
    }

    onMount(loadAll);
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">Batch</h1>
            <p class="text-zinc-500 mt-1">Compute environments, queues, and jobs.</p>
        </div>
    </div>

    <div class="flex gap-1 mb-6 border-b border-zinc-800">
        <button onclick={() => activeTab = 'envs'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'envs' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Compute Environments ({envs.length})</button>
        <button onclick={() => activeTab = 'queues'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'queues' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Job Queues ({queues.length})</button>
        <button onclick={() => activeTab = 'jobs'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'jobs' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Jobs ({jobs.length})</button>
    </div>

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if activeTab === 'envs'}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">Name</th><th class="px-4 py-3 text-xs">Type</th><th class="px-4 py-3 text-xs">State</th><th class="px-4 py-3 text-xs">Status</th></tr></thead>
                <tbody>
                    {#each envs as e}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 text-zinc-200">{e.computeEnvironmentName}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs">{e.type}</td>
                            <td class="px-4 py-3"><span class="px-1.5 py-0.5 rounded text-xs {statusColor(e.state)}">{e.state}</span></td>
                            <td class="px-4 py-3"><span class="px-1.5 py-0.5 rounded text-xs {statusColor(e.status)}">{e.status}</span></td>
                        </tr>
                    {/each}
                    {#if envs.length === 0}
                        <tr><td colspan="4" class="px-4 py-8 text-center text-zinc-500">No compute environments.</td></tr>
                    {/if}
                </tbody>
            </table>
        </div>
    {:else if activeTab === 'queues'}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">Name</th><th class="px-4 py-3 text-xs">Priority</th><th class="px-4 py-3 text-xs">State</th><th class="px-4 py-3 text-xs">Status</th></tr></thead>
                <tbody>
                    {#each queues as q}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 text-zinc-200">{q.jobQueueName}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs">{q.priority}</td>
                            <td class="px-4 py-3"><span class="px-1.5 py-0.5 rounded text-xs {statusColor(q.state)}">{q.state}</span></td>
                            <td class="px-4 py-3"><span class="px-1.5 py-0.5 rounded text-xs {statusColor(q.status)}">{q.status}</span></td>
                        </tr>
                    {/each}
                    {#if queues.length === 0}
                        <tr><td colspan="4" class="px-4 py-8 text-center text-zinc-500">No job queues.</td></tr>
                    {/if}
                </tbody>
            </table>
        </div>
    {:else}
        <div>
            <div class="flex items-center justify-between mb-4">
                <span class="text-sm text-zinc-400">{jobs.length} job{jobs.length !== 1 ? 's' : ''}</span>
                <button onclick={() => { showSubmit = !showSubmit; submitError = null; }} class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">Submit Job</button>
            </div>
            {#if showSubmit}
                <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                    <h3 class="font-semibold mb-3">Submit Job</h3>
                    {#if submitError}<div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{submitError}</div>{/if}
                    <div class="space-y-3 mb-3">
                        <input type="text" bind:value={jobName} placeholder="Job name" class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" />
                        <input type="text" bind:value={jobQueue} placeholder="Job queue name/arn" class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" />
                        <input type="text" bind:value={jobDefinition} placeholder="Job definition name:revision" class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" />
                    </div>
                    <div class="flex gap-2">
                        <button onclick={submitJob} disabled={submitting} class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm font-medium">{submitting ? 'Submitting...' : 'Submit'}</button>
                        <button onclick={() => showSubmit = false} class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm">Cancel</button>
                    </div>
                </div>
            {/if}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">Job ID</th><th class="px-4 py-3 text-xs">Name</th><th class="px-4 py-3 text-xs">Status</th><th class="px-4 py-3 text-xs"></th></tr></thead>
                    <tbody>
                        {#each jobs as j}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400 text-xs">{j.jobId}</td>
                                <td class="px-4 py-3 text-zinc-200">{j.jobName}</td>
                                <td class="px-4 py-3"><span class="px-1.5 py-0.5 rounded text-xs {statusColor(j.status)}">{j.status}</span></td>
                                <td class="px-4 py-3"><button onclick={() => terminateJob(j.jobId)} class="text-red-400 hover:text-red-300 text-xs">Terminate</button></td>
                            </tr>
                        {/each}
                        {#if jobs.length === 0}
                            <tr><td colspan="4" class="px-4 py-8 text-center text-zinc-500">No jobs.</td></tr>
                        {/if}
                    </tbody>
                </table>
            </div>
        </div>
    {/if}
</div>
