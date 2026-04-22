<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listStateMachines, createStateMachine, deleteStateMachine,
        listExecutions, startExecution, describeStateMachine,
        getExecutionHistory, describeExecution,
        type StateMachine, type SfnExecution, type SfnHistoryEvent,
    } from '$lib/aws';
    import AslViewer from '$lib/components/AslViewer.svelte';

    let stateMachines = $state<StateMachine[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let showCreateForm = $state(false);
    let newMachineName = $state('');
    let newDefinition = $state(JSON.stringify({
        Comment: 'A simple state machine',
        StartAt: 'HelloWorld',
        States: {
            HelloWorld: {
                Type: 'Pass',
                Result: 'Hello, World!',
                End: true,
            },
        },
    }, null, 2));
    let creating = $state(false);
    let createError = $state<string | null>(null);
    let confirmDeleteMachine = $state<string | null>(null);

    let selectedMachine = $state<StateMachine | null>(null);
    let machineDefinition = $state<string | null>(null);
    let machineDefLoading = $state(false);
    let showRawJson = $state(false);

    let executions = $state<SfnExecution[]>([]);
    let executionsLoading = $state(false);
    let showStartExecution = $state(false);
    let executionInput = $state('{}');
    let startingExecution = $state(false);
    let startExecutionError = $state<string | null>(null);

    let selectedExecution = $state<SfnExecution | null>(null);
    let executionDetail = $state<{ status: string; input?: string; output?: string; startDate: number; stopDate?: number; name: string } | null>(null);
    let executionHistory = $state<SfnHistoryEvent[]>([]);
    let executionDetailLoading = $state(false);

    async function loadStateMachines() {
        loading = true;
        error = null;
        try {
            const data = await listStateMachines();
            stateMachines = data.stateMachines;
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to load state machines';
        } finally {
            loading = false;
        }
    }

    async function handleCreateMachine() {
        if (!newMachineName.trim() || !newDefinition.trim()) return;
        creating = true;
        createError = null;
        try {
            await createStateMachine(newMachineName.trim(), newDefinition.trim());
            newMachineName = '';
            showCreateForm = false;
            await loadStateMachines();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed to create state machine';
        } finally {
            creating = false;
        }
    }

    async function handleDeleteMachine(arn: string) {
        try {
            await deleteStateMachine(arn);
            confirmDeleteMachine = null;
            if (selectedMachine?.arn === arn) {
                selectedMachine = null;
                machineDefinition = null;
                executions = [];
                selectedExecution = null;
                executionDetail = null;
                executionHistory = [];
            }
            await loadStateMachines();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete state machine';
        }
    }

    async function selectMachine(machine: StateMachine) {
        selectedMachine = machine;
        showStartExecution = false;
        selectedExecution = null;
        executionDetail = null;
        executionHistory = [];
        machineDefinition = null;
        showRawJson = false;
        await Promise.all([
            loadExecutions(machine.arn),
            loadMachineDefinition(machine.arn),
        ]);
    }

    async function loadMachineDefinition(arn: string) {
        machineDefLoading = true;
        try {
            const data = await describeStateMachine(arn);
            machineDefinition = data.definition;
        } catch {
            machineDefinition = null;
        } finally {
            machineDefLoading = false;
        }
    }

    async function loadExecutions(arn: string) {
        executionsLoading = true;
        executions = [];
        try {
            const data = await listExecutions(arn);
            executions = data.executions;
        } catch {
            // silently fail
        } finally {
            executionsLoading = false;
        }
    }

    async function handleStartExecution() {
        if (!selectedMachine) return;
        startingExecution = true;
        startExecutionError = null;
        try {
            await startExecution(selectedMachine.arn, executionInput.trim() || '{}');
            showStartExecution = false;
            executionInput = '{}';
            await loadExecutions(selectedMachine.arn);
        } catch (e) {
            startExecutionError = e instanceof Error ? e.message : 'Failed to start execution';
        } finally {
            startingExecution = false;
        }
    }

    async function selectExecution(exec: SfnExecution) {
        selectedExecution = exec;
        executionDetail = null;
        executionHistory = [];
        executionDetailLoading = true;
        try {
            const [detail, history] = await Promise.all([
                describeExecution(exec.arn),
                getExecutionHistory(exec.arn),
            ]);
            executionDetail = detail;
            executionHistory = history.events;
        } catch {
            // silently fail
        } finally {
            executionDetailLoading = false;
        }
    }

    function executionStatusColor(status: string): string {
        if (status === 'SUCCEEDED') return 'bg-green-900/40 text-green-400';
        if (status === 'FAILED' || status === 'TIMED_OUT' || status === 'ABORTED') return 'bg-red-900/40 text-red-400';
        if (status === 'RUNNING') return 'bg-yellow-900/40 text-yellow-400';
        return 'bg-zinc-800 text-zinc-400';
    }

    function historyEventColor(type: string): string {
        if (type.includes('Succeeded') || type === 'ExecutionSucceeded') return 'text-green-400';
        if (type.includes('Failed') || type.includes('Aborted') || type === 'ExecutionFailed') return 'text-red-400';
        if (type.includes('Started') || type.includes('Entered')) return 'text-blue-400';
        if (type.includes('Exited')) return 'text-orange-400';
        return 'text-zinc-400';
    }

    function formatDate(iso: string): string {
        try { return new Date(iso).toLocaleString(); } catch { return iso; }
    }

    function formatTimestamp(ts: number): string {
        try { return new Date(ts).toLocaleString(); } catch { return String(ts); }
    }

    function prettyJson(str?: string): string {
        if (!str) return '';
        try { return JSON.stringify(JSON.parse(str), null, 2); } catch { return str; }
    }

    onMount(() => loadStateMachines());
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">Step Functions — State Machines</h1>
            <p class="text-zinc-500 mt-1">Coordinate distributed applications using visual workflows.</p>
        </div>
        <div class="flex items-center gap-3">
            <span class="text-sm text-zinc-500">{stateMachines.length} state machine{stateMachines.length !== 1 ? 's' : ''}</span>
            <button
                onclick={() => { showCreateForm = !showCreateForm; createError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create State Machine
            </button>
        </div>
    </div>

    {#if showCreateForm}
        <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
            <h3 class="font-semibold mb-3">Create State Machine</h3>
            {#if createError}
                <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createError}</div>
            {/if}
            <div class="mb-3">
                <label for="machine-name" class="block text-xs text-zinc-400 mb-1">Name</label>
                <input
                    id="machine-name"
                    type="text"
                    bind:value={newMachineName}
                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                    placeholder="my-state-machine"
                />
            </div>
            <div class="mb-3">
                <label for="machine-def" class="block text-xs text-zinc-400 mb-1">ASL Definition (JSON)</label>
                <textarea
                    id="machine-def"
                    bind:value={newDefinition}
                    rows="12"
                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y"
                ></textarea>
            </div>
            <div class="flex gap-2">
                <button
                    onclick={handleCreateMachine}
                    disabled={creating || !newMachineName.trim() || !newDefinition.trim()}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                >
                    {creating ? 'Creating...' : 'Create'}
                </button>
                <button
                    onclick={() => { showCreateForm = false; createError = null; newMachineName = ''; }}
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
    {:else if stateMachines.length === 0 && !showCreateForm}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No state machines yet.</p>
            <button onclick={() => showCreateForm = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                Create your first state machine
            </button>
        </div>
    {:else}
        <div class="flex gap-4">
            <!-- Machine list -->
            <div class="w-72 shrink-0">
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                    {#each stateMachines as machine}
                        <div class="border-b border-zinc-800/50 last:border-0 {selectedMachine?.arn === machine.arn ? 'bg-zinc-800' : 'hover:bg-zinc-800/40'} transition-colors">
                            <div class="px-4 py-3">
                                <button class="w-full text-left" onclick={() => selectMachine(machine)}>
                                    <div class="font-mono text-orange-400 text-sm truncate">{machine.name}</div>
                                    <div class="text-xs text-zinc-500 mt-0.5">{machine.type}</div>
                                    <div class="text-xs text-zinc-600 mt-0.5">{formatDate(machine.creationDate)}</div>
                                </button>
                                <div class="mt-2 flex justify-end">
                                    {#if confirmDeleteMachine === machine.arn}
                                        <div class="flex items-center gap-1">
                                            <button onclick={() => handleDeleteMachine(machine.arn)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteMachine = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button onclick={() => confirmDeleteMachine = machine.arn} class="px-2 py-0.5 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                    {/if}
                                </div>
                            </div>
                        </div>
                    {/each}
                </div>
            </div>

            <!-- Detail panel -->
            <div class="flex-1 min-w-0 flex flex-col gap-4">
                {#if selectedMachine}
                    <!-- Workflow diagram + Start Execution -->
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                        <div class="px-4 py-3 border-b border-zinc-800 flex items-center justify-between">
                            <div>
                                <span class="font-mono text-orange-400">{selectedMachine.name}</span>
                                <span class="ml-2 text-xs text-zinc-500">{selectedMachine.type}</span>
                            </div>
                            <button
                                onclick={() => { showStartExecution = !showStartExecution; startExecutionError = null; }}
                                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
                            >
                                Start Execution
                            </button>
                        </div>

                        {#if showStartExecution}
                            <div class="p-4 border-b border-zinc-800 bg-zinc-800/30">
                                <h4 class="text-sm font-medium mb-2">Start Execution</h4>
                                {#if startExecutionError}
                                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs mb-2">{startExecutionError}</div>
                                {/if}
                                <label for="exec-input" class="block text-xs text-zinc-400 mb-1">Input JSON</label>
                                <textarea
                                    id="exec-input"
                                    bind:value={executionInput}
                                    rows="4"
                                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y"
                                ></textarea>
                                <div class="flex gap-2 mt-2">
                                    <button
                                        onclick={handleStartExecution}
                                        disabled={startingExecution}
                                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                                    >
                                        {startingExecution ? 'Starting...' : 'Start'}
                                    </button>
                                    <button
                                        onclick={() => { showStartExecution = false; startExecutionError = null; executionInput = '{}'; }}
                                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                                    >
                                        Cancel
                                    </button>
                                </div>
                            </div>
                        {/if}

                        <!-- Visual ASL diagram and raw JSON side by side -->
                        <div class="p-4">
                            {#if machineDefLoading}
                                <div class="text-zinc-500 text-sm">Loading definition...</div>
                            {:else if machineDefinition}
                                <div class="flex gap-6 items-start">
                                    <div class="shrink-0">
                                        <div class="text-xs text-zinc-500 mb-2 font-medium uppercase tracking-wide">Workflow Diagram</div>
                                        <AslViewer definition={machineDefinition} />
                                    </div>
                                    <div class="flex-1 min-w-0">
                                        <button
                                            class="text-xs text-zinc-500 hover:text-zinc-300 font-medium uppercase tracking-wide mb-2 flex items-center gap-1 transition-colors"
                                            onclick={() => showRawJson = !showRawJson}
                                        >
                                            {showRawJson ? 'Hide' : 'Show'} ASL JSON
                                            <span class="text-zinc-600">{showRawJson ? '▲' : '▼'}</span>
                                        </button>
                                        {#if showRawJson}
                                            <pre class="bg-zinc-950 border border-zinc-800 rounded p-3 text-xs font-mono text-zinc-300 overflow-auto max-h-96">{prettyJson(machineDefinition)}</pre>
                                        {/if}
                                    </div>
                                </div>
                            {:else}
                                <div class="text-zinc-600 text-sm">Could not load ASL definition.</div>
                            {/if}
                        </div>
                    </div>

                    <!-- Executions list -->
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                        <div class="px-4 py-3 border-b border-zinc-800">
                            <h3 class="text-sm font-medium text-zinc-300">Executions</h3>
                        </div>
                        <div class="px-4 py-3">
                            {#if executionsLoading}
                                <div class="text-zinc-500 text-sm">Loading executions...</div>
                            {:else if executions.length === 0}
                                <div class="text-zinc-500 text-sm">No executions yet. Start one above.</div>
                            {:else}
                                <table class="w-full text-sm">
                                    <thead>
                                        <tr class="text-left text-zinc-500 border-b border-zinc-800">
                                            <th class="pb-2 pr-4">Name</th>
                                            <th class="pb-2 pr-4">Status</th>
                                            <th class="pb-2 pr-4">Started</th>
                                            <th class="pb-2">Stopped</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {#each executions as exec}
                                            <tr
                                                class="border-b border-zinc-800/50 cursor-pointer {selectedExecution?.arn === exec.arn ? 'bg-zinc-800' : 'hover:bg-zinc-800/30'} transition-colors"
                                                onclick={() => selectExecution(exec)}
                                            >
                                                <td class="py-2 pr-4 font-mono text-orange-400 text-xs truncate max-w-xs">{exec.name}</td>
                                                <td class="py-2 pr-4">
                                                    <span class="px-1.5 py-0.5 rounded text-xs {executionStatusColor(exec.status)}">{exec.status}</span>
                                                </td>
                                                <td class="py-2 pr-4 text-zinc-400 text-xs whitespace-nowrap">{formatDate(exec.startDate)}</td>
                                                <td class="py-2 text-zinc-500 text-xs whitespace-nowrap">{exec.stopDate ? formatDate(exec.stopDate) : '—'}</td>
                                            </tr>
                                        {/each}
                                    </tbody>
                                </table>
                            {/if}
                        </div>
                    </div>

                    <!-- Execution history panel -->
                    {#if selectedExecution}
                        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                            <div class="px-4 py-3 border-b border-zinc-800 flex items-center justify-between">
                                <div>
                                    <span class="text-sm font-medium text-zinc-300">Execution Detail</span>
                                    <span class="ml-2 font-mono text-xs text-orange-400">{selectedExecution.name}</span>
                                </div>
                                {#if executionDetail}
                                    <span class="px-2 py-0.5 rounded text-xs {executionStatusColor(executionDetail.status)}">{executionDetail.status}</span>
                                {/if}
                            </div>

                            {#if executionDetailLoading}
                                <div class="px-4 py-3 text-zinc-500 text-sm">Loading execution details...</div>
                            {:else}
                                {#if executionDetail}
                                    <!-- Input/Output summary -->
                                    <div class="px-4 py-3 border-b border-zinc-800 grid grid-cols-2 gap-4">
                                        <div>
                                            <div class="text-xs text-zinc-500 mb-1 font-medium">Input</div>
                                            <pre class="bg-zinc-950 border border-zinc-800 rounded p-2 text-xs font-mono text-zinc-300 overflow-auto max-h-32">{prettyJson(executionDetail.input) || '—'}</pre>
                                        </div>
                                        <div>
                                            <div class="text-xs text-zinc-500 mb-1 font-medium">Output</div>
                                            <pre class="bg-zinc-950 border border-zinc-800 rounded p-2 text-xs font-mono text-zinc-300 overflow-auto max-h-32">{prettyJson(executionDetail.output) || '—'}</pre>
                                        </div>
                                    </div>
                                {/if}

                                <!-- Event timeline -->
                                {#if executionHistory.length > 0}
                                    <div class="px-4 py-3">
                                        <div class="text-xs text-zinc-500 mb-3 font-medium uppercase tracking-wide">State Transition Timeline</div>
                                        <div class="space-y-1">
                                            {#each executionHistory as event}
                                                <div class="flex items-start gap-3 py-1.5 px-2 rounded hover:bg-zinc-800/40 transition-colors">
                                                    <div class="w-2 h-2 rounded-full mt-1.5 shrink-0 {event.type.includes('Succeeded') || event.type === 'ExecutionSucceeded' ? 'bg-green-400' : event.type.includes('Failed') || event.type.includes('Aborted') ? 'bg-red-400' : event.type.includes('Entered') || event.type.includes('Started') ? 'bg-blue-400' : 'bg-zinc-500'}"></div>
                                                    <div class="flex-1 min-w-0">
                                                        <div class="flex items-center gap-2 flex-wrap">
                                                            <span class="text-xs font-mono {historyEventColor(event.type)}">{event.type}</span>
                                                            {#if event.stateEnteredEventDetails?.name || event.stateExitedEventDetails?.name}
                                                                <span class="text-xs text-zinc-400 bg-zinc-800 px-1.5 py-0.5 rounded">
                                                                    {event.stateEnteredEventDetails?.name ?? event.stateExitedEventDetails?.name}
                                                                </span>
                                                            {/if}
                                                            <span class="text-xs text-zinc-600 ml-auto">{formatTimestamp(event.timestamp)}</span>
                                                        </div>
                                                        {#if event.stateEnteredEventDetails?.input}
                                                            <pre class="mt-1 text-xs font-mono text-zinc-500 truncate">{prettyJson(event.stateEnteredEventDetails.input).slice(0, 120)}{prettyJson(event.stateEnteredEventDetails.input).length > 120 ? '...' : ''}</pre>
                                                        {/if}
                                                        {#if event.stateExitedEventDetails?.output}
                                                            <pre class="mt-1 text-xs font-mono text-zinc-500 truncate">{prettyJson(event.stateExitedEventDetails.output).slice(0, 120)}{prettyJson(event.stateExitedEventDetails.output).length > 120 ? '...' : ''}</pre>
                                                        {/if}
                                                        {#if event.taskFailedEventDetails}
                                                            <div class="mt-1 text-xs text-red-400">{event.taskFailedEventDetails.error}: {event.taskFailedEventDetails.cause}</div>
                                                        {/if}
                                                    </div>
                                                </div>
                                            {/each}
                                        </div>
                                    </div>
                                {:else}
                                    <div class="px-4 py-3 text-zinc-500 text-sm">No events found for this execution.</div>
                                {/if}
                            {/if}
                        </div>
                    {/if}

                {:else}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-500 text-sm">
                        Select a state machine to view its workflow and executions.
                    </div>
                {/if}
            </div>
        </div>
    {/if}
</div>
