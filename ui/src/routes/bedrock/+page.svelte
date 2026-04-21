<script lang="ts">
    import { onMount } from 'svelte';

    const BASE = 'http://localhost:4566';

    interface FoundationModel {
        modelId: string;
        modelName: string;
        providerName: string;
        inputModalities: string[];
        outputModalities: string[];
    }

    interface Guardrail {
        guardrailId: string;
        name: string;
        guardrailArn: string;
        status: string;
        createdAt: string;
        version: string;
    }

    let models = $state<FoundationModel[]>([]);
    let guardrails = $state<Guardrail[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);
    let activeTab = $state<'models' | 'guardrails'>('models');

    let selectedModel = $state<FoundationModel | null>(null);

    let showGuardrailForm = $state(false);
    let gName = $state('');
    let gBlockedInput = $state('This input is not allowed.');
    let gBlockedOutput = $state('This output is not allowed.');
    let creating = $state(false);
    let createError = $state<string | null>(null);
    let confirmDeleteGuardrail = $state<string | null>(null);

    async function apiFetch(path: string, opts?: RequestInit) {
        const signingService = path.startsWith('/model/') ? 'bedrock-runtime' : 'bedrock';
        const res = await fetch(`${BASE}${path}`, {
            headers: {
                'Content-Type': 'application/json',
                'Authorization': `AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/${signingService}/aws4_request, SignedHeaders=host, Signature=fake`,
            },
            ...opts,
        });
        if (!res.ok) {
            const text = await res.text();
            throw new Error(text || `HTTP ${res.status}`);
        }
        const text = await res.text();
        return text ? JSON.parse(text) : {};
    }

    async function loadModels() {
        const data = await apiFetch('/foundation-models');
        models = data.modelSummaries ?? [];
    }

    async function loadGuardrails() {
        const data = await apiFetch('/guardrails');
        guardrails = data.guardrails ?? [];
    }

    async function loadAll() {
        loading = true;
        error = null;
        try {
            await Promise.all([loadModels(), loadGuardrails()]);
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to load data';
        } finally {
            loading = false;
        }
    }

    async function handleCreateGuardrail() {
        if (!gName.trim()) return;
        creating = true;
        createError = null;
        try {
            await apiFetch('/guardrails', {
                method: 'POST',
                body: JSON.stringify({
                    name: gName.trim(),
                    blockedInputMessaging: gBlockedInput,
                    blockedOutputsMessaging: gBlockedOutput,
                }),
            });
            gName = '';
            showGuardrailForm = false;
            await loadGuardrails();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed to create guardrail';
        } finally {
            creating = false;
        }
    }

    async function handleDeleteGuardrail(id: string) {
        try {
            await apiFetch(`/guardrails/${id}`, { method: 'DELETE' });
            confirmDeleteGuardrail = null;
            await loadGuardrails();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete guardrail';
        }
    }

    function providerColor(provider: string): string {
        if (provider === 'Anthropic') return 'text-purple-400';
        if (provider === 'Amazon') return 'text-orange-400';
        if (provider === 'Meta') return 'text-blue-400';
        if (provider === 'Cohere') return 'text-green-400';
        if (provider === 'Stability AI') return 'text-pink-400';
        return 'text-zinc-400';
    }

    function modalityBadge(m: string): string {
        if (m === 'TEXT') return 'bg-blue-900/40 text-blue-300';
        if (m === 'IMAGE') return 'bg-pink-900/40 text-pink-300';
        if (m === 'EMBEDDING') return 'bg-green-900/40 text-green-300';
        return 'bg-zinc-800 text-zinc-400';
    }

    onMount(() => loadAll());
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">Bedrock — Foundation Models</h1>
            <p class="text-zinc-500 mt-1">Access foundation models and manage guardrails.</p>
        </div>
    </div>

    <!-- Tabs -->
    <div class="flex gap-1 mb-6 border-b border-zinc-800">
        <button
            onclick={() => activeTab = 'models'}
            class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'models' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}"
        >
            Foundation Models
        </button>
        <button
            onclick={() => activeTab = 'guardrails'}
            class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'guardrails' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}"
        >
            Guardrails ({guardrails.length})
        </button>
    </div>

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if activeTab === 'models'}
        <div class="flex gap-4">
            <!-- Model list -->
            <div class="flex-1 min-w-0">
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                    <table class="w-full text-sm">
                        <thead>
                            <tr class="border-b border-zinc-800 text-left text-zinc-500">
                                <th class="px-4 py-3 text-xs">Model ID</th>
                                <th class="px-4 py-3 text-xs">Name</th>
                                <th class="px-4 py-3 text-xs">Provider</th>
                                <th class="px-4 py-3 text-xs">Modalities</th>
                            </tr>
                        </thead>
                        <tbody>
                            {#each models as model}
                                <tr
                                    class="border-b border-zinc-800/50 hover:bg-zinc-800/30 cursor-pointer {selectedModel?.modelId === model.modelId ? 'bg-zinc-800/50' : ''}"
                                    onclick={() => selectedModel = selectedModel?.modelId === model.modelId ? null : model}
                                >
                                    <td class="px-4 py-3 font-mono text-orange-400 text-xs">{model.modelId}</td>
                                    <td class="px-4 py-3 text-zinc-300 text-xs">{model.modelName}</td>
                                    <td class="px-4 py-3 text-xs {providerColor(model.providerName)}">{model.providerName}</td>
                                    <td class="px-4 py-3">
                                        <div class="flex gap-1 flex-wrap">
                                            {#each model.inputModalities as m}
                                                <span class="px-1.5 py-0.5 rounded text-xs {modalityBadge(m)}">{m}</span>
                                            {/each}
                                        </div>
                                    </td>
                                </tr>
                            {/each}
                        </tbody>
                    </table>
                </div>
            </div>

            <!-- Model detail -->
            {#if selectedModel}
                <div class="w-64 shrink-0">
                    <div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4 sticky top-4">
                        <div class="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-3">Model Details</div>
                        <div class="space-y-3">
                            <div>
                                <div class="text-xs text-zinc-500 mb-0.5">Name</div>
                                <div class="text-sm text-zinc-200">{selectedModel.modelName}</div>
                            </div>
                            <div>
                                <div class="text-xs text-zinc-500 mb-0.5">Provider</div>
                                <div class="text-sm {providerColor(selectedModel.providerName)}">{selectedModel.providerName}</div>
                            </div>
                            <div>
                                <div class="text-xs text-zinc-500 mb-1">Model ID</div>
                                <div class="font-mono text-xs text-orange-400 break-all">{selectedModel.modelId}</div>
                            </div>
                            <div>
                                <div class="text-xs text-zinc-500 mb-1">Input Modalities</div>
                                <div class="flex gap-1 flex-wrap">
                                    {#each selectedModel.inputModalities as m}
                                        <span class="px-1.5 py-0.5 rounded text-xs {modalityBadge(m)}">{m}</span>
                                    {/each}
                                </div>
                            </div>
                            <div class="pt-2 text-xs text-zinc-600 border-t border-zinc-800">
                                Use with bedrock-runtime InvokeModel or Converse API.
                            </div>
                        </div>
                    </div>
                </div>
            {/if}
        </div>
    {:else}
        <!-- Guardrails tab -->
        <div>
            <div class="flex items-center justify-between mb-4">
                <span class="text-sm text-zinc-400">{guardrails.length} guardrail{guardrails.length !== 1 ? 's' : ''}</span>
                <button
                    onclick={() => { showGuardrailForm = !showGuardrailForm; createError = null; }}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
                >
                    Create Guardrail
                </button>
            </div>

            {#if showGuardrailForm}
                <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                    <h3 class="font-semibold mb-3">Create Guardrail</h3>
                    {#if createError}
                        <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createError}</div>
                    {/if}
                    <div class="space-y-3 mb-3">
                        <div>
                            <label for="g-name" class="block text-xs text-zinc-400 mb-1">Name</label>
                            <input
                                id="g-name"
                                type="text"
                                bind:value={gName}
                                class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                                placeholder="my-guardrail"
                            />
                        </div>
                        <div>
                            <label for="g-blocked-input" class="block text-xs text-zinc-400 mb-1">Blocked Input Message</label>
                            <input
                                id="g-blocked-input"
                                type="text"
                                bind:value={gBlockedInput}
                                class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                            />
                        </div>
                        <div>
                            <label for="g-blocked-output" class="block text-xs text-zinc-400 mb-1">Blocked Output Message</label>
                            <input
                                id="g-blocked-output"
                                type="text"
                                bind:value={gBlockedOutput}
                                class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                            />
                        </div>
                    </div>
                    <div class="flex gap-2">
                        <button
                            onclick={handleCreateGuardrail}
                            disabled={creating || !gName.trim()}
                            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                        >
                            {creating ? 'Creating...' : 'Create'}
                        </button>
                        <button
                            onclick={() => { showGuardrailForm = false; createError = null; gName = ''; }}
                            class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                        >
                            Cancel
                        </button>
                    </div>
                </div>
            {/if}

            {#if guardrails.length === 0 && !showGuardrailForm}
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                    <p class="text-zinc-500">No guardrails yet.</p>
                    <button onclick={() => showGuardrailForm = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                        Create your first guardrail
                    </button>
                </div>
            {:else if guardrails.length > 0}
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                    <table class="w-full text-sm">
                        <thead>
                            <tr class="border-b border-zinc-800 text-left text-zinc-500">
                                <th class="px-4 py-3 text-xs">Name</th>
                                <th class="px-4 py-3 text-xs">ID</th>
                                <th class="px-4 py-3 text-xs">Status</th>
                                <th class="px-4 py-3 text-xs">Version</th>
                                <th class="px-4 py-3 text-xs"></th>
                            </tr>
                        </thead>
                        <tbody>
                            {#each guardrails as g}
                                <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                    <td class="px-4 py-3 text-zinc-200 text-sm">{g.name}</td>
                                    <td class="px-4 py-3 font-mono text-orange-400 text-xs">{g.guardrailId}</td>
                                    <td class="px-4 py-3">
                                        <span class="px-1.5 py-0.5 rounded text-xs bg-green-900/40 text-green-400">{g.status}</span>
                                    </td>
                                    <td class="px-4 py-3 text-zinc-400 text-xs">{g.version}</td>
                                    <td class="px-4 py-3">
                                        {#if confirmDeleteGuardrail === g.guardrailId}
                                            <div class="flex items-center gap-1">
                                                <button onclick={() => handleDeleteGuardrail(g.guardrailId)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                                <button onclick={() => confirmDeleteGuardrail = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                            </div>
                                        {:else}
                                            <button onclick={() => confirmDeleteGuardrail = g.guardrailId} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                        {/if}
                                    </td>
                                </tr>
                            {/each}
                        </tbody>
                    </table>
                </div>
            {/if}
        </div>
    {/if}
</div>
