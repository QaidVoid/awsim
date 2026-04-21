<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listStacks, createStack, deleteStack,
        describeStackResources, describeStackEvents,
        type CfStack, type CfStackResource, type CfStackEvent,
    } from '$lib/aws';

    let stacks = $state<CfStack[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let showCreateForm = $state(false);
    let newStackName = $state('');
    let newTemplateBody = $state(`AWSTemplateFormatVersion: '2010-09-09'
Description: My stack
Resources:
  MyBucket:
    Type: AWS::S3::Bucket
`);
    let creating = $state(false);
    let createError = $state<string | null>(null);
    let confirmDeleteStack = $state<string | null>(null);

    let selectedStack = $state<CfStack | null>(null);
    let stackResources = $state<CfStackResource[]>([]);
    let stackEvents = $state<CfStackEvent[]>([]);
    let detailLoading = $state(false);
    let detailTab = $state<'resources' | 'events'>('resources');

    function statusColor(status: string): string {
        if (status.includes('FAILED')) return 'bg-red-900/40 text-red-400';
        if (status.includes('IN_PROGRESS')) return 'bg-yellow-900/40 text-yellow-400';
        if (status === 'CREATE_COMPLETE' || status === 'UPDATE_COMPLETE') return 'bg-green-900/40 text-green-400';
        if (status.includes('DELETE')) return 'bg-zinc-800 text-zinc-500';
        return 'bg-zinc-800 text-zinc-400';
    }

    async function loadStacks() {
        loading = true;
        error = null;
        try {
            const data = await listStacks();
            stacks = data.stacks;
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to load stacks';
        } finally {
            loading = false;
        }
    }

    async function handleCreateStack() {
        if (!newStackName.trim() || !newTemplateBody.trim()) return;
        creating = true;
        createError = null;
        try {
            await createStack(newStackName.trim(), newTemplateBody.trim());
            newStackName = '';
            showCreateForm = false;
            await loadStacks();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed to create stack';
        } finally {
            creating = false;
        }
    }

    async function handleDeleteStack(name: string) {
        try {
            await deleteStack(name);
            confirmDeleteStack = null;
            if (selectedStack?.stackName === name) selectedStack = null;
            await loadStacks();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete stack';
        }
    }

    async function selectStack(stack: CfStack) {
        selectedStack = stack;
        detailLoading = true;
        try {
            const [res, ev] = await Promise.all([
                describeStackResources(stack.stackName),
                describeStackEvents(stack.stackName),
            ]);
            stackResources = res.resources;
            stackEvents = ev.events;
        } catch {
            stackResources = [];
            stackEvents = [];
        } finally {
            detailLoading = false;
        }
    }

    function formatDate(iso: string): string {
        try { return new Date(iso).toLocaleString(); } catch { return iso; }
    }

    onMount(() => loadStacks());
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">CloudFormation — Stacks</h1>
            <p class="text-zinc-500 mt-1">Infrastructure as code. Model and provision AWS resources using templates.</p>
        </div>
        <div class="flex items-center gap-3">
            <span class="text-sm text-zinc-500">{stacks.length} stack{stacks.length !== 1 ? 's' : ''}</span>
            <button
                onclick={() => { showCreateForm = !showCreateForm; createError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Stack
            </button>
        </div>
    </div>

    {#if showCreateForm}
        <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
            <h3 class="font-semibold mb-3">Create Stack</h3>
            {#if createError}
                <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createError}</div>
            {/if}
            <div class="mb-3">
                <label for="stack-name" class="block text-xs text-zinc-400 mb-1">Stack Name</label>
                <input
                    id="stack-name"
                    type="text"
                    bind:value={newStackName}
                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                    placeholder="my-stack"
                />
            </div>
            <div class="mb-3">
                <label for="stack-template" class="block text-xs text-zinc-400 mb-1">Template Body (YAML or JSON)</label>
                <textarea
                    id="stack-template"
                    bind:value={newTemplateBody}
                    rows="10"
                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y"
                ></textarea>
            </div>
            <div class="flex gap-2">
                <button
                    onclick={handleCreateStack}
                    disabled={creating || !newStackName.trim() || !newTemplateBody.trim()}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                >
                    {creating ? 'Creating...' : 'Create'}
                </button>
                <button
                    onclick={() => { showCreateForm = false; createError = null; newStackName = ''; }}
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
    {:else if stacks.length === 0 && !showCreateForm}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No CloudFormation stacks yet.</p>
            <button onclick={() => showCreateForm = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                Create your first stack
            </button>
        </div>
    {:else}
        <div class="flex gap-4">
            <!-- Stack list -->
            <div class="w-80 shrink-0">
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                    {#each stacks as stack}
                        <div class="border-b border-zinc-800/50 last:border-0 {selectedStack?.stackName === stack.stackName ? 'bg-zinc-800' : 'hover:bg-zinc-800/40'} transition-colors">
                            <div class="px-4 py-3">
                                <button class="w-full text-left" onclick={() => selectStack(stack)}>
                                    <div class="font-mono text-orange-400 text-sm truncate">{stack.stackName}</div>
                                    <div class="flex items-center gap-2 mt-1">
                                        <span class="px-1.5 py-0.5 rounded text-xs {statusColor(stack.stackStatus)}">{stack.stackStatus}</span>
                                        <span class="text-xs text-zinc-500">{formatDate(stack.creationTime)}</span>
                                    </div>
                                    {#if stack.description}
                                        <div class="text-xs text-zinc-600 mt-0.5 truncate">{stack.description}</div>
                                    {/if}
                                </button>
                                <div class="mt-2 flex justify-end">
                                    {#if confirmDeleteStack === stack.stackName}
                                        <div class="flex items-center gap-1">
                                            <button onclick={() => handleDeleteStack(stack.stackName)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteStack = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button onclick={(e) => { e.stopPropagation(); confirmDeleteStack = stack.stackName; }} class="px-2 py-0.5 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                    {/if}
                                </div>
                            </div>
                        </div>
                    {/each}
                </div>
            </div>

            <!-- Stack detail -->
            <div class="flex-1 min-w-0">
                {#if selectedStack}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                        <div class="px-4 py-3 border-b border-zinc-800 flex items-center justify-between">
                            <div>
                                <span class="font-mono text-orange-400">{selectedStack.stackName}</span>
                                <span class="ml-2 px-1.5 py-0.5 rounded text-xs {statusColor(selectedStack.stackStatus)}">{selectedStack.stackStatus}</span>
                            </div>
                            <div class="flex gap-1">
                                <button
                                    onclick={() => detailTab = 'resources'}
                                    class="px-3 py-1 text-xs rounded {detailTab === 'resources' ? 'bg-zinc-700 text-zinc-200' : 'text-zinc-500 hover:text-zinc-300'}"
                                >
                                    Resources
                                </button>
                                <button
                                    onclick={() => detailTab = 'events'}
                                    class="px-3 py-1 text-xs rounded {detailTab === 'events' ? 'bg-zinc-700 text-zinc-200' : 'text-zinc-500 hover:text-zinc-300'}"
                                >
                                    Events
                                </button>
                            </div>
                        </div>

                        {#if detailLoading}
                            <div class="p-4 text-zinc-500 text-sm">Loading...</div>
                        {:else if detailTab === 'resources'}
                            {#if stackResources.length === 0}
                                <div class="p-4 text-zinc-500 text-sm">No resources found.</div>
                            {:else}
                                <table class="w-full text-sm">
                                    <thead>
                                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                                            <th class="px-4 py-2">Logical ID</th>
                                            <th class="px-4 py-2">Physical ID</th>
                                            <th class="px-4 py-2">Type</th>
                                            <th class="px-4 py-2">Status</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {#each stackResources as res}
                                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                                <td class="px-4 py-2 font-mono text-orange-400 text-xs">{res.logicalResourceId}</td>
                                                <td class="px-4 py-2 text-zinc-400 text-xs font-mono truncate max-w-xs">{res.physicalResourceId}</td>
                                                <td class="px-4 py-2 text-zinc-400 text-xs">{res.resourceType}</td>
                                                <td class="px-4 py-2">
                                                    <span class="px-1.5 py-0.5 rounded text-xs {statusColor(res.resourceStatus)}">{res.resourceStatus}</span>
                                                </td>
                                            </tr>
                                        {/each}
                                    </tbody>
                                </table>
                            {/if}
                        {:else}
                            {#if stackEvents.length === 0}
                                <div class="p-4 text-zinc-500 text-sm">No events found.</div>
                            {:else}
                                <table class="w-full text-sm">
                                    <thead>
                                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                                            <th class="px-4 py-2">Time</th>
                                            <th class="px-4 py-2">Logical ID</th>
                                            <th class="px-4 py-2">Type</th>
                                            <th class="px-4 py-2">Status</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {#each stackEvents as ev}
                                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                                <td class="px-4 py-2 text-zinc-500 text-xs whitespace-nowrap">{formatDate(ev.timestamp)}</td>
                                                <td class="px-4 py-2 font-mono text-orange-400 text-xs">{ev.logicalResourceId}</td>
                                                <td class="px-4 py-2 text-zinc-400 text-xs">{ev.resourceType}</td>
                                                <td class="px-4 py-2">
                                                    <span class="px-1.5 py-0.5 rounded text-xs {statusColor(ev.resourceStatus)}">{ev.resourceStatus}</span>
                                                </td>
                                            </tr>
                                        {/each}
                                    </tbody>
                                </table>
                            {/if}
                        {/if}
                    </div>
                {:else}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-500 text-sm">
                        Select a stack to view resources and events.
                    </div>
                {/if}
            </div>
        </div>
    {/if}
</div>
