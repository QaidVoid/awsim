<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listEventBuses, listRules, putRule, deleteRule, putEvents,
        type EventBus, type EventRule,
    } from '$lib/aws';

    // --- Event buses ---
    let buses = $state<EventBus[]>([]);
    let busesLoading = $state(true);
    let busesError = $state<string | null>(null);
    let selectedBus = $state<string | null>(null);

    // --- Rules ---
    let rules = $state<EventRule[]>([]);
    let rulesLoading = $state(false);
    let rulesError = $state<string | null>(null);
    let confirmDeleteRule = $state<string | null>(null);

    // --- Create rule form ---
    let showCreateRule = $state(false);
    let newRuleName = $state('');
    let newRulePattern = $state(JSON.stringify({ source: ['my.app'] }, null, 2));
    let creatingRule = $state(false);
    let createRuleError = $state<string | null>(null);

    // --- Put Event form ---
    let eventSource = $state('my.app');
    let eventDetailType = $state('MyEvent');
    let eventDetail = $state(JSON.stringify({ key: 'value' }, null, 2));
    let sendingEvent = $state(false);
    let sendEventResult = $state<string | null>(null);
    let sendEventError = $state<string | null>(null);

    async function loadBuses() {
        busesLoading = true;
        busesError = null;
        try {
            const data = await listEventBuses();
            buses = data.eventBuses;
        } catch (e) {
            busesError = e instanceof Error ? e.message : 'Failed to load event buses';
        } finally {
            busesLoading = false;
        }
    }

    async function selectBus(name: string) {
        selectedBus = name;
        showCreateRule = false;
        confirmDeleteRule = null;
        sendEventResult = null;
        sendEventError = null;
        await loadRules(name);
    }

    async function loadRules(busName: string) {
        rulesLoading = true;
        rulesError = null;
        try {
            const data = await listRules(busName);
            rules = data.rules;
        } catch (e) {
            rulesError = e instanceof Error ? e.message : 'Failed to load rules';
        } finally {
            rulesLoading = false;
        }
    }

    async function handleCreateRule() {
        if (!newRuleName.trim() || !selectedBus) return;
        creatingRule = true;
        createRuleError = null;
        try {
            await putRule(newRuleName.trim(), selectedBus, newRulePattern);
            newRuleName = '';
            newRulePattern = JSON.stringify({ source: ['my.app'] }, null, 2);
            showCreateRule = false;
            await loadRules(selectedBus);
        } catch (e) {
            createRuleError = e instanceof Error ? e.message : 'Failed to create rule';
        } finally {
            creatingRule = false;
        }
    }

    async function handleDeleteRule(ruleName: string) {
        if (!selectedBus) return;
        try {
            await deleteRule(ruleName, selectedBus);
            confirmDeleteRule = null;
            await loadRules(selectedBus);
        } catch (e) {
            rulesError = e instanceof Error ? e.message : 'Failed to delete rule';
        }
    }

    async function handlePutEvent() {
        if (!eventSource.trim() || !eventDetailType.trim() || !selectedBus) return;
        sendingEvent = true;
        sendEventResult = null;
        sendEventError = null;
        try {
            await putEvents([{
                Source: eventSource.trim(),
                DetailType: eventDetailType.trim(),
                Detail: eventDetail,
                EventBusName: selectedBus,
            }]);
            sendEventResult = 'Event sent successfully.';
        } catch (e) {
            sendEventError = e instanceof Error ? e.message : 'Failed to send event';
        } finally {
            sendingEvent = false;
        }
    }

    function formatDate(iso: string): string {
        try { return new Date(iso).toLocaleString(); } catch { return iso; }
    }

    onMount(loadBuses);
</script>

<div class="p-6">
    <div class="mb-6">
        <h1 class="text-2xl font-bold">EventBridge — Event Buses &amp; Rules</h1>
        <p class="text-zinc-500 mt-1">Serverless event bus. Route events between services using rules.</p>
    </div>

    {#if busesLoading}
        <div class="text-zinc-500">Loading...</div>
    {:else if busesError}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{busesError}</div>
    {:else}
        <div class="flex gap-4">
            <!-- Left: Event bus list -->
            <div class="w-64 shrink-0">
                <div class="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2 px-1">Event Buses</div>
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                    {#each buses as bus}
                        <button
                            onclick={() => selectBus(bus.name)}
                            class="w-full text-left px-4 py-3 border-b border-zinc-800/50 last:border-0 transition-colors {selectedBus === bus.name ? 'bg-zinc-800' : 'hover:bg-zinc-800/40'}"
                        >
                            <div class="font-mono text-sm text-orange-400 truncate">{bus.name}</div>
                            <div class="text-xs text-zinc-600 mt-0.5 truncate font-mono">{bus.arn.split(':').slice(-1)[0]}</div>
                        </button>
                    {:else}
                        <div class="px-4 py-6 text-center text-zinc-600 text-sm">No event buses found.</div>
                    {/each}
                </div>
            </div>

            <!-- Right: Rules + forms -->
            <div class="flex-1 min-w-0">
                {#if selectedBus}
                    <!-- Put Event form -->
                    <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                        <h3 class="font-semibold mb-3 text-sm">Put Event to <span class="text-orange-400 font-mono">{selectedBus}</span></h3>
                        {#if sendEventResult}
                            <div class="bg-green-900/20 border border-green-800 rounded p-2 text-green-400 text-sm mb-3">{sendEventResult}</div>
                        {/if}
                        {#if sendEventError}
                            <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{sendEventError}</div>
                        {/if}
                        <div class="grid grid-cols-2 gap-3 mb-3">
                            <div>
                                <label for="event-source" class="block text-xs text-zinc-400 mb-1">Source</label>
                                <input
                                    id="event-source"
                                    type="text"
                                    bind:value={eventSource}
                                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                                    placeholder="my.app"
                                />
                            </div>
                            <div>
                                <label for="event-detail-type" class="block text-xs text-zinc-400 mb-1">Detail Type</label>
                                <input
                                    id="event-detail-type"
                                    type="text"
                                    bind:value={eventDetailType}
                                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                                    placeholder="MyEvent"
                                />
                            </div>
                        </div>
                        <div class="mb-3">
                            <label for="event-detail" class="block text-xs text-zinc-400 mb-1">Detail (JSON)</label>
                            <textarea
                                id="event-detail"
                                bind:value={eventDetail}
                                rows="3"
                                class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y"
                            ></textarea>
                        </div>
                        <button
                            onclick={handlePutEvent}
                            disabled={sendingEvent || !eventSource.trim() || !eventDetailType.trim()}
                            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                        >
                            {sendingEvent ? 'Sending...' : 'Send Event'}
                        </button>
                    </div>

                    <!-- Rules header -->
                    <div class="flex items-center justify-between mb-3">
                        <div class="text-sm font-medium text-zinc-300">
                            Rules
                            {#if !rulesLoading}
                                <span class="text-zinc-600 font-normal ml-1">({rules.length})</span>
                            {/if}
                        </div>
                        <button
                            onclick={() => { showCreateRule = !showCreateRule; createRuleError = null; }}
                            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
                        >
                            Create Rule
                        </button>
                    </div>

                    <!-- Create rule form -->
                    {#if showCreateRule}
                        <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                            <h3 class="font-semibold mb-3 text-sm">Create Rule</h3>
                            {#if createRuleError}
                                <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createRuleError}</div>
                            {/if}
                            <div class="mb-3">
                                <label for="rule-name" class="block text-xs text-zinc-400 mb-1">Rule Name</label>
                                <input
                                    id="rule-name"
                                    type="text"
                                    bind:value={newRuleName}
                                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                                    placeholder="my-rule"
                                />
                            </div>
                            <div class="mb-3">
                                <label for="rule-pattern" class="block text-xs text-zinc-400 mb-1">Event Pattern (JSON)</label>
                                <textarea
                                    id="rule-pattern"
                                    bind:value={newRulePattern}
                                    rows="5"
                                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y"
                                ></textarea>
                            </div>
                            <div class="flex gap-2">
                                <button
                                    onclick={handleCreateRule}
                                    disabled={creatingRule || !newRuleName.trim()}
                                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                                >
                                    {creatingRule ? 'Creating...' : 'Create'}
                                </button>
                                <button
                                    onclick={() => { showCreateRule = false; createRuleError = null; newRuleName = ''; }}
                                    class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                                >
                                    Cancel
                                </button>
                            </div>
                        </div>
                    {/if}

                    <!-- Rules list -->
                    {#if rulesLoading}
                        <div class="text-zinc-500 text-sm">Loading rules...</div>
                    {:else if rulesError}
                        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400 text-sm">{rulesError}</div>
                    {:else if rules.length === 0}
                        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-600 text-sm">
                            No rules on this bus.
                        </div>
                    {:else}
                        <div class="space-y-2">
                            {#each rules as rule}
                                <div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4">
                                    <div class="flex items-start justify-between gap-3">
                                        <div class="min-w-0">
                                            <div class="flex items-center gap-2 mb-1">
                                                <span class="font-mono text-sm text-orange-400">{rule.name}</span>
                                                <span class="px-1.5 py-0.5 rounded text-xs font-medium {rule.state === 'ENABLED' ? 'bg-green-900/30 text-green-400' : 'bg-red-900/30 text-red-400'}">
                                                    {rule.state}
                                                </span>
                                            </div>
                                            {#if rule.eventPattern}
                                                <div class="text-xs text-zinc-500 font-mono bg-zinc-800 rounded px-2 py-1 mt-2 overflow-x-auto">
                                                    {rule.eventPattern}
                                                </div>
                                            {/if}
                                        </div>
                                        <div class="shrink-0">
                                            {#if confirmDeleteRule === rule.name}
                                                <div class="flex items-center gap-1">
                                                    <button
                                                        onclick={() => handleDeleteRule(rule.name)}
                                                        class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs"
                                                    >Confirm</button>
                                                    <button
                                                        onclick={() => confirmDeleteRule = null}
                                                        class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs"
                                                    >Cancel</button>
                                                </div>
                                            {:else}
                                                <button
                                                    onclick={() => confirmDeleteRule = rule.name}
                                                    class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors"
                                                >Delete</button>
                                            {/if}
                                        </div>
                                    </div>
                                </div>
                            {/each}
                        </div>
                    {/if}
                {:else}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-500 text-sm">
                        Select an event bus to manage rules.
                    </div>
                {/if}
            </div>
        </div>
    {/if}
</div>
