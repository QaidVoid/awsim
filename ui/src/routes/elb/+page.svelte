<script lang="ts">
    import { onMount } from 'svelte';
    import {
        describeLoadBalancers, createLoadBalancer, deleteLoadBalancer,
        describeTargetGroups, createTargetGroup, deleteTargetGroup,
        type ElbLoadBalancer, type ElbTargetGroup,
    } from '$lib/aws';

    let activeTab = $state<'loadBalancers' | 'targetGroups'>('loadBalancers');

    // ---- Load Balancers ----
    let loadBalancers = $state<ElbLoadBalancer[]>([]);
    let lbLoading = $state(false);
    let lbError = $state<string | null>(null);
    let showCreateLb = $state(false);
    let newLbName = $state('');
    let newLbType = $state('application');
    let newLbScheme = $state('internet-facing');
    let creatingLb = $state(false);
    let createLbError = $state<string | null>(null);
    let confirmDeleteLb = $state<string | null>(null);

    // ---- Target Groups ----
    let targetGroups = $state<ElbTargetGroup[]>([]);
    let tgLoading = $state(false);
    let tgError = $state<string | null>(null);
    let showCreateTg = $state(false);
    let newTgName = $state('');
    let newTgProtocol = $state('HTTP');
    let newTgPort = $state(80);
    let newTgTargetType = $state('instance');
    let creatingTg = $state(false);
    let createTgError = $state<string | null>(null);
    let confirmDeleteTg = $state<string | null>(null);

    async function loadLoadBalancers() {
        lbLoading = true;
        lbError = null;
        try {
            const data = await describeLoadBalancers();
            loadBalancers = data.loadBalancers;
        } catch (e) {
            lbError = e instanceof Error ? e.message : 'Failed to load load balancers';
        } finally {
            lbLoading = false;
        }
    }

    async function handleCreateLb() {
        if (!newLbName.trim()) return;
        creatingLb = true;
        createLbError = null;
        try {
            await createLoadBalancer(newLbName.trim(), newLbType, newLbScheme);
            showCreateLb = false;
            newLbName = '';
            await loadLoadBalancers();
        } catch (e) {
            createLbError = e instanceof Error ? e.message : 'Failed to create load balancer';
        } finally {
            creatingLb = false;
        }
    }

    async function handleDeleteLb(arn: string) {
        try {
            await deleteLoadBalancer(arn);
            confirmDeleteLb = null;
            await loadLoadBalancers();
        } catch (e) {
            lbError = e instanceof Error ? e.message : 'Failed to delete load balancer';
        }
    }

    async function loadTargetGroups() {
        tgLoading = true;
        tgError = null;
        try {
            const data = await describeTargetGroups();
            targetGroups = data.targetGroups;
        } catch (e) {
            tgError = e instanceof Error ? e.message : 'Failed to load target groups';
        } finally {
            tgLoading = false;
        }
    }

    async function handleCreateTg() {
        if (!newTgName.trim()) return;
        creatingTg = true;
        createTgError = null;
        try {
            await createTargetGroup(newTgName.trim(), newTgProtocol, newTgPort, newTgTargetType);
            showCreateTg = false;
            newTgName = '';
            await loadTargetGroups();
        } catch (e) {
            createTgError = e instanceof Error ? e.message : 'Failed to create target group';
        } finally {
            creatingTg = false;
        }
    }

    async function handleDeleteTg(arn: string) {
        try {
            await deleteTargetGroup(arn);
            confirmDeleteTg = null;
            await loadTargetGroups();
        } catch (e) {
            tgError = e instanceof Error ? e.message : 'Failed to delete target group';
        }
    }

    function switchTab(tab: 'loadBalancers' | 'targetGroups') {
        activeTab = tab;
        if (tab === 'loadBalancers' && loadBalancers.length === 0 && !lbLoading) loadLoadBalancers();
        if (tab === 'targetGroups' && targetGroups.length === 0 && !tgLoading) loadTargetGroups();
    }

    onMount(() => loadLoadBalancers());
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">ELB — Elastic Load Balancing v2</h1>
            <p class="text-zinc-500 mt-1">Manage Application and Network Load Balancers, Target Groups, and Listeners.</p>
        </div>
        {#if activeTab === 'loadBalancers'}
            <button
                onclick={() => { showCreateLb = !showCreateLb; createLbError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Load Balancer
            </button>
        {:else if activeTab === 'targetGroups'}
            <button
                onclick={() => { showCreateTg = !showCreateTg; createTgError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Target Group
            </button>
        {/if}
    </div>

    <!-- Tab nav -->
    <div class="flex gap-1 mb-4 border-b border-zinc-800">
        {#each [['loadBalancers', 'Load Balancers'], ['targetGroups', 'Target Groups']] as [tab, label]}
            <button
                onclick={() => switchTab(tab as 'loadBalancers' | 'targetGroups')}
                class="px-4 py-2 text-sm font-medium border-b-2 transition-colors {activeTab === tab ? 'border-orange-400 text-orange-400' : 'border-transparent text-zinc-500 hover:text-zinc-300'}"
            >
                {label}
            </button>
        {/each}
    </div>

    <!-- Load Balancers tab -->
    {#if activeTab === 'loadBalancers'}
        {#if showCreateLb}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create Load Balancer</h3>
                {#if createLbError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createLbError}</div>
                {/if}
                <div class="grid grid-cols-3 gap-3 mb-3">
                    <div>
                        <label for="lb-name" class="block text-xs text-zinc-400 mb-1">Name</label>
                        <input id="lb-name" type="text" bind:value={newLbName}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                            placeholder="my-load-balancer" />
                    </div>
                    <div>
                        <label for="lb-type" class="block text-xs text-zinc-400 mb-1">Type</label>
                        <select id="lb-type" bind:value={newLbType}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500">
                            <option value="application">Application (ALB)</option>
                            <option value="network">Network (NLB)</option>
                        </select>
                    </div>
                    <div>
                        <label for="lb-scheme" class="block text-xs text-zinc-400 mb-1">Scheme</label>
                        <select id="lb-scheme" bind:value={newLbScheme}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500">
                            <option value="internet-facing">Internet-facing</option>
                            <option value="internal">Internal</option>
                        </select>
                    </div>
                </div>
                <div class="flex gap-2">
                    <button
                        onclick={handleCreateLb}
                        disabled={creatingLb || !newLbName.trim()}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                    >
                        {creatingLb ? 'Creating...' : 'Create'}
                    </button>
                    <button
                        onclick={() => { showCreateLb = false; createLbError = null; newLbName = ''; }}
                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        {/if}

        {#if lbLoading}
            <div class="text-zinc-500">Loading load balancers...</div>
        {:else if lbError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{lbError}</div>
        {:else if loadBalancers.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No load balancers found.</p>
                <button onclick={() => showCreateLb = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                    Create your first load balancer
                </button>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Name</th>
                            <th class="px-4 py-3">DNS Name</th>
                            <th class="px-4 py-3">Type</th>
                            <th class="px-4 py-3">Scheme</th>
                            <th class="px-4 py-3">State</th>
                            <th class="px-4 py-3"></th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each loadBalancers as lb}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-semibold text-zinc-200">{lb.name}</td>
                                <td class="px-4 py-3 font-mono text-zinc-400 text-xs">{lb.dnsName}</td>
                                <td class="px-4 py-3">
                                    <span class="px-2 py-0.5 rounded text-xs {lb.type === 'application' ? 'bg-blue-900/40 text-blue-400' : 'bg-purple-900/40 text-purple-400'}">{lb.type}</span>
                                </td>
                                <td class="px-4 py-3 text-zinc-400 text-xs">{lb.scheme}</td>
                                <td class="px-4 py-3">
                                    <span class="px-2 py-0.5 rounded text-xs bg-green-900/40 text-green-400">{lb.state}</span>
                                </td>
                                <td class="px-4 py-3">
                                    {#if confirmDeleteLb === lb.arn}
                                        <div class="flex items-center gap-1">
                                            <button onclick={() => handleDeleteLb(lb.arn)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteLb = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button onclick={() => confirmDeleteLb = lb.arn} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                    {/if}
                                </td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}

    <!-- Target Groups tab -->
    {#if activeTab === 'targetGroups'}
        {#if showCreateTg}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create Target Group</h3>
                {#if createTgError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createTgError}</div>
                {/if}
                <div class="grid grid-cols-4 gap-3 mb-3">
                    <div>
                        <label for="tg-name" class="block text-xs text-zinc-400 mb-1">Name</label>
                        <input id="tg-name" type="text" bind:value={newTgName}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                            placeholder="my-targets" />
                    </div>
                    <div>
                        <label for="tg-protocol" class="block text-xs text-zinc-400 mb-1">Protocol</label>
                        <select id="tg-protocol" bind:value={newTgProtocol}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500">
                            <option value="HTTP">HTTP</option>
                            <option value="HTTPS">HTTPS</option>
                            <option value="TCP">TCP</option>
                        </select>
                    </div>
                    <div>
                        <label for="tg-port" class="block text-xs text-zinc-400 mb-1">Port</label>
                        <input id="tg-port" type="number" bind:value={newTgPort}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                            min="1" max="65535" />
                    </div>
                    <div>
                        <label for="tg-type" class="block text-xs text-zinc-400 mb-1">Target Type</label>
                        <select id="tg-type" bind:value={newTgTargetType}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500">
                            <option value="instance">Instance</option>
                            <option value="ip">IP</option>
                            <option value="lambda">Lambda</option>
                        </select>
                    </div>
                </div>
                <div class="flex gap-2">
                    <button
                        onclick={handleCreateTg}
                        disabled={creatingTg || !newTgName.trim()}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                    >
                        {creatingTg ? 'Creating...' : 'Create'}
                    </button>
                    <button
                        onclick={() => { showCreateTg = false; createTgError = null; newTgName = ''; }}
                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        {/if}

        {#if tgLoading}
            <div class="text-zinc-500">Loading target groups...</div>
        {:else if tgError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{tgError}</div>
        {:else if targetGroups.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No target groups found.</p>
                <button onclick={() => showCreateTg = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                    Create your first target group
                </button>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Name</th>
                            <th class="px-4 py-3">Protocol</th>
                            <th class="px-4 py-3">Port</th>
                            <th class="px-4 py-3">VPC</th>
                            <th class="px-4 py-3">Type</th>
                            <th class="px-4 py-3"></th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each targetGroups as tg}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-semibold text-zinc-200">{tg.name}</td>
                                <td class="px-4 py-3 text-zinc-400">{tg.protocol}</td>
                                <td class="px-4 py-3 text-zinc-400">{tg.port}</td>
                                <td class="px-4 py-3 font-mono text-zinc-400 text-xs">{tg.vpcId || '-'}</td>
                                <td class="px-4 py-3 text-zinc-400 text-xs">{tg.targetType}</td>
                                <td class="px-4 py-3">
                                    {#if confirmDeleteTg === tg.arn}
                                        <div class="flex items-center gap-1">
                                            <button onclick={() => handleDeleteTg(tg.arn)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteTg = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button onclick={() => confirmDeleteTg = tg.arn} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                    {/if}
                                </td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}
</div>
