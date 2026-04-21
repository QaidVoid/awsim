<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listFunctions, createFunction, deleteFunction, invokeFunction,
        type LambdaFunction,
    } from '$lib/aws';

    let functions = $state<LambdaFunction[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let selectedFn = $state<LambdaFunction | null>(null);
    let invokePayload = $state('{}');
    let invoking = $state(false);
    let invokeResult = $state<string | null>(null);
    let invokeError = $state<string | null>(null);

    let showCreateForm = $state(false);
    let newFnName = $state('');
    let newFnRuntime = $state('python3.11');
    let newFnHandler = $state('index.handler');
    let newFnRoleArn = $state('arn:aws:iam::000000000000:role/exec');
    let newFnCode = $state('');
    let creating = $state(false);
    let createError = $state<string | null>(null);

    let confirmDeleteFn = $state<string | null>(null);

    const runtimes = [
        'python3.11', 'python3.10', 'python3.9',
        'nodejs20.x', 'nodejs18.x',
        'java21', 'java17',
        'go1.x',
        'dotnet8',
    ];

    async function loadFunctions() {
        loading = true;
        error = null;
        try {
            const data = await listFunctions();
            functions = data.functions;
        } catch {
            error = 'Could not connect to AWSim. Is it running on port 4566?';
        } finally {
            loading = false;
        }
    }

    async function handleCreateFunction() {
        if (!newFnName.trim()) return;
        creating = true;
        createError = null;
        try {
            await createFunction(newFnName.trim(), newFnRuntime, newFnHandler, newFnRoleArn, newFnCode);
            newFnName = '';
            newFnCode = '';
            showCreateForm = false;
            await loadFunctions();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed to create function';
        } finally {
            creating = false;
        }
    }

    async function handleDeleteFunction(name: string) {
        try {
            await deleteFunction(name);
            confirmDeleteFn = null;
            if (selectedFn?.name === name) selectedFn = null;
            await loadFunctions();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete function';
        }
    }

    async function handleInvoke() {
        if (!selectedFn) return;
        invoking = true;
        invokeResult = null;
        invokeError = null;
        try {
            const result = await invokeFunction(selectedFn.name, invokePayload);
            invokeResult = result;
        } catch (e) {
            invokeError = e instanceof Error ? e.message : 'Invocation failed';
        } finally {
            invoking = false;
        }
    }

    function selectFn(fn: LambdaFunction) {
        selectedFn = fn;
        invokeResult = null;
        invokeError = null;
        invokePayload = '{}';
    }

    function formatDate(iso: string): string {
        try { return new Date(iso).toLocaleString(); } catch { return iso; }
    }

    function formatResult(raw: string): string {
        try { return JSON.stringify(JSON.parse(raw), null, 2); } catch { return raw; }
    }

    onMount(loadFunctions);
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">Lambda — Functions</h1>
            <p class="text-zinc-500 mt-1">Serverless compute. Run code without provisioning servers.</p>
        </div>
        <div class="flex items-center gap-3">
            <span class="text-sm text-zinc-500">{functions.length} function{functions.length !== 1 ? 's' : ''}</span>
            <button
                onclick={() => { showCreateForm = !showCreateForm; createError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Function
            </button>
        </div>
    </div>

    {#if showCreateForm}
        <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
            <h3 class="font-semibold mb-3">Create Function</h3>
            {#if createError}
                <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createError}</div>
            {/if}
            <div class="grid grid-cols-2 gap-3 mb-3">
                <div>
                    <label for="fn-name" class="block text-xs text-zinc-400 mb-1">Function Name</label>
                    <input id="fn-name" type="text" bind:value={newFnName} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" placeholder="my-function" />
                </div>
                <div>
                    <label for="fn-runtime" class="block text-xs text-zinc-400 mb-1">Runtime</label>
                    <select id="fn-runtime" bind:value={newFnRuntime} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500">
                        {#each runtimes as rt}
                            <option value={rt}>{rt}</option>
                        {/each}
                    </select>
                </div>
                <div>
                    <label for="fn-handler" class="block text-xs text-zinc-400 mb-1">Handler</label>
                    <input id="fn-handler" type="text" bind:value={newFnHandler} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" placeholder="index.handler" />
                </div>
                <div>
                    <label for="fn-role" class="block text-xs text-zinc-400 mb-1">Execution Role ARN</label>
                    <input id="fn-role" type="text" bind:value={newFnRoleArn} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" placeholder="arn:aws:iam::000000000000:role/exec" />
                </div>
            </div>
            <div class="mb-3">
                <label for="fn-code" class="block text-xs text-zinc-400 mb-1">Code (Base64 ZIP, optional)</label>
                <input id="fn-code" type="text" bind:value={newFnCode} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500" placeholder="base64-encoded zip contents" />
            </div>
            <div class="flex gap-2">
                <button
                    onclick={handleCreateFunction}
                    disabled={creating || !newFnName.trim()}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                >
                    {creating ? 'Creating...' : 'Create'}
                </button>
                <button
                    onclick={() => { showCreateForm = false; createError = null; newFnName = ''; }}
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
    {:else if functions.length === 0 && !showCreateForm}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No functions yet.</p>
            <button onclick={() => showCreateForm = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                Create your first function
            </button>
        </div>
    {:else}
        <div class="flex gap-4">
            <!-- Function list -->
            <div class="w-72 shrink-0">
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                    {#each functions as fn}
                        <div class="border-b border-zinc-800/50 last:border-0 {selectedFn?.name === fn.name ? 'bg-zinc-800' : 'hover:bg-zinc-800/40'} transition-colors">
                            <div class="px-4 py-3 flex items-start justify-between gap-2">
                                <button class="flex-1 text-left min-w-0" onclick={() => selectFn(fn)}>
                                    <div class="font-mono text-orange-400 text-sm truncate">{fn.name}</div>
                                    <div class="text-xs text-zinc-500 mt-0.5">{fn.runtime} &middot; {fn.memory} MB</div>
                                    <div class="text-xs text-zinc-600 mt-0.5">{formatDate(fn.lastModified)}</div>
                                </button>
                                <button
                                    onclick={(e) => { e.stopPropagation(); confirmDeleteFn = fn.name; }}
                                    class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs shrink-0 transition-colors"
                                >
                                    Delete
                                </button>
                            </div>
                            {#if confirmDeleteFn === fn.name}
                                <div class="px-4 pb-3 bg-red-900/10 border-t border-red-900/30">
                                    <p class="text-xs text-red-400 mb-2">Delete "{fn.name}"?</p>
                                    <div class="flex gap-2">
                                        <button onclick={() => handleDeleteFunction(fn.name)} class="px-2 py-1 bg-red-700 hover:bg-red-600 rounded text-xs font-medium">Confirm</button>
                                        <button onclick={() => confirmDeleteFn = null} class="px-2 py-1 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                    </div>
                                </div>
                            {/if}
                        </div>
                    {/each}
                </div>
            </div>

            <!-- Detail / invoke panel -->
            <div class="flex-1 min-w-0">
                {#if selectedFn}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                        <div class="px-4 py-3 border-b border-zinc-800">
                            <h2 class="font-semibold text-orange-400 font-mono">{selectedFn.name}</h2>
                        </div>
                        <div class="p-4 grid grid-cols-2 gap-x-8 gap-y-2 text-sm border-b border-zinc-800">
                            <div>
                                <span class="text-zinc-500">Runtime</span>
                                <span class="ml-2 text-zinc-200">{selectedFn.runtime}</span>
                            </div>
                            <div>
                                <span class="text-zinc-500">Memory</span>
                                <span class="ml-2 text-zinc-200">{selectedFn.memory} MB</span>
                            </div>
                            <div>
                                <span class="text-zinc-500">Handler</span>
                                <span class="ml-2 font-mono text-zinc-200">{selectedFn.handler}</span>
                            </div>
                            <div>
                                <span class="text-zinc-500">Last Modified</span>
                                <span class="ml-2 text-zinc-200">{formatDate(selectedFn.lastModified)}</span>
                            </div>
                        </div>

                        <!-- Invoke -->
                        <div class="p-4">
                            <h3 class="text-sm font-semibold mb-2">Invoke</h3>
                            <label for="invoke-payload" class="block text-xs text-zinc-400 mb-1">Payload (JSON)</label>
                            <textarea
                                id="invoke-payload"
                                bind:value={invokePayload}
                                rows="5"
                                class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y mb-3"
                            ></textarea>
                            <button
                                onclick={handleInvoke}
                                disabled={invoking}
                                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm font-medium transition-colors"
                            >
                                {invoking ? 'Invoking...' : 'Invoke'}
                            </button>

                            {#if invokeError}
                                <div class="mt-3 bg-red-900/20 border border-red-800 rounded p-3 text-red-400 text-sm">{invokeError}</div>
                            {/if}
                            {#if invokeResult !== null}
                                <div class="mt-3">
                                    <div class="text-xs text-zinc-500 mb-1">Result</div>
                                    <pre class="bg-zinc-800 rounded p-3 text-xs font-mono text-zinc-200 overflow-auto max-h-64">{formatResult(invokeResult)}</pre>
                                </div>
                            {/if}
                        </div>
                    </div>
                {:else}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-500 text-sm">
                        Select a function to view details and invoke it.
                    </div>
                {/if}
            </div>
        </div>
    {/if}
</div>
