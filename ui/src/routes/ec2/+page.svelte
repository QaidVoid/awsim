<script lang="ts">
    import { onMount } from 'svelte';
    import {
        describeVpcs, createVpc, deleteVpc,
        describeSubnets,
        describeSecurityGroups, createSecurityGroup, deleteSecurityGroup,
        type Ec2Vpc, type Ec2Subnet, type Ec2SecurityGroup,
    } from '$lib/aws';

    let activeTab = $state<'vpcs' | 'subnets' | 'securityGroups'>('vpcs');

    // ---- VPCs ----
    let vpcs = $state<Ec2Vpc[]>([]);
    let vpcsLoading = $state(false);
    let vpcsError = $state<string | null>(null);
    let showCreateVpc = $state(false);
    let newVpcCidr = $state('10.0.0.0/16');
    let creatingVpc = $state(false);
    let createVpcError = $state<string | null>(null);
    let confirmDeleteVpc = $state<string | null>(null);

    // ---- Subnets ----
    let subnets = $state<Ec2Subnet[]>([]);
    let subnetsLoading = $state(false);
    let subnetsError = $state<string | null>(null);

    // ---- Security Groups ----
    let securityGroups = $state<Ec2SecurityGroup[]>([]);
    let sgLoading = $state(false);
    let sgError = $state<string | null>(null);
    let showCreateSg = $state(false);
    let newSgName = $state('');
    let newSgDesc = $state('');
    let newSgVpcId = $state('');
    let creatingSg = $state(false);
    let createSgError = $state<string | null>(null);
    let confirmDeleteSg = $state<string | null>(null);

    async function loadVpcs() {
        vpcsLoading = true;
        vpcsError = null;
        try {
            const data = await describeVpcs();
            vpcs = data.vpcs;
        } catch (e) {
            vpcsError = e instanceof Error ? e.message : 'Failed to load VPCs';
        } finally {
            vpcsLoading = false;
        }
    }

    async function handleCreateVpc() {
        if (!newVpcCidr.trim()) return;
        creatingVpc = true;
        createVpcError = null;
        try {
            await createVpc(newVpcCidr.trim());
            showCreateVpc = false;
            await loadVpcs();
        } catch (e) {
            createVpcError = e instanceof Error ? e.message : 'Failed to create VPC';
        } finally {
            creatingVpc = false;
        }
    }

    async function handleDeleteVpc(vpcId: string) {
        try {
            await deleteVpc(vpcId);
            confirmDeleteVpc = null;
            await loadVpcs();
        } catch (e) {
            vpcsError = e instanceof Error ? e.message : 'Failed to delete VPC';
        }
    }

    async function loadSubnets() {
        subnetsLoading = true;
        subnetsError = null;
        try {
            const data = await describeSubnets();
            subnets = data.subnets;
        } catch (e) {
            subnetsError = e instanceof Error ? e.message : 'Failed to load subnets';
        } finally {
            subnetsLoading = false;
        }
    }

    async function loadSecurityGroups() {
        sgLoading = true;
        sgError = null;
        try {
            const data = await describeSecurityGroups();
            securityGroups = data.securityGroups;
        } catch (e) {
            sgError = e instanceof Error ? e.message : 'Failed to load security groups';
        } finally {
            sgLoading = false;
        }
    }

    async function handleCreateSg() {
        if (!newSgName.trim() || !newSgDesc.trim() || !newSgVpcId.trim()) return;
        creatingSg = true;
        createSgError = null;
        try {
            await createSecurityGroup(newSgName.trim(), newSgDesc.trim(), newSgVpcId.trim());
            newSgName = '';
            newSgDesc = '';
            newSgVpcId = '';
            showCreateSg = false;
            await loadSecurityGroups();
        } catch (e) {
            createSgError = e instanceof Error ? e.message : 'Failed to create security group';
        } finally {
            creatingSg = false;
        }
    }

    async function handleDeleteSg(groupId: string) {
        try {
            await deleteSecurityGroup(groupId);
            confirmDeleteSg = null;
            await loadSecurityGroups();
        } catch (e) {
            sgError = e instanceof Error ? e.message : 'Failed to delete security group';
        }
    }

    function switchTab(tab: 'vpcs' | 'subnets' | 'securityGroups') {
        activeTab = tab;
        if (tab === 'vpcs' && vpcs.length === 0 && !vpcsLoading) loadVpcs();
        if (tab === 'subnets' && subnets.length === 0 && !subnetsLoading) loadSubnets();
        if (tab === 'securityGroups' && securityGroups.length === 0 && !sgLoading) loadSecurityGroups();
    }

    onMount(() => loadVpcs());
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">EC2 — Elastic Compute Cloud</h1>
            <p class="text-zinc-500 mt-1">Manage VPCs, subnets, and security groups.</p>
        </div>
        {#if activeTab === 'vpcs'}
            <button
                onclick={() => { showCreateVpc = !showCreateVpc; createVpcError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create VPC
            </button>
        {:else if activeTab === 'securityGroups'}
            <button
                onclick={() => { showCreateSg = !showCreateSg; createSgError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Security Group
            </button>
        {/if}
    </div>

    <!-- Tab nav -->
    <div class="flex gap-1 mb-4 border-b border-zinc-800">
        {#each [['vpcs', 'VPCs'], ['subnets', 'Subnets'], ['securityGroups', 'Security Groups']] as [tab, label]}
            <button
                onclick={() => switchTab(tab as 'vpcs' | 'subnets' | 'securityGroups')}
                class="px-4 py-2 text-sm font-medium border-b-2 transition-colors {activeTab === tab ? 'border-orange-400 text-orange-400' : 'border-transparent text-zinc-500 hover:text-zinc-300'}"
            >
                {label}
            </button>
        {/each}
    </div>

    <!-- VPCs tab -->
    {#if activeTab === 'vpcs'}
        {#if showCreateVpc}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create VPC</h3>
                {#if createVpcError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createVpcError}</div>
                {/if}
                <label for="vpc-cidr" class="block text-xs text-zinc-400 mb-1">CIDR Block</label>
                <input
                    id="vpc-cidr"
                    type="text"
                    bind:value={newVpcCidr}
                    onkeydown={(e) => e.key === 'Enter' && handleCreateVpc()}
                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                    placeholder="10.0.0.0/16"
                />
                <div class="flex gap-2 mt-3">
                    <button
                        onclick={handleCreateVpc}
                        disabled={creatingVpc || !newVpcCidr.trim()}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                    >
                        {creatingVpc ? 'Creating...' : 'Create'}
                    </button>
                    <button
                        onclick={() => { showCreateVpc = false; createVpcError = null; newVpcCidr = '10.0.0.0/16'; }}
                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        {/if}

        {#if vpcsLoading}
            <div class="text-zinc-500">Loading VPCs...</div>
        {:else if vpcsError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{vpcsError}</div>
        {:else if vpcs.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No VPCs found.</p>
                <button onclick={() => showCreateVpc = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                    Create your first VPC
                </button>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">VPC ID</th>
                            <th class="px-4 py-3">CIDR Block</th>
                            <th class="px-4 py-3">State</th>
                            <th class="px-4 py-3">Default</th>
                            <th class="px-4 py-3"></th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each vpcs as vpc}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400 text-xs">{vpc.vpcId}</td>
                                <td class="px-4 py-3 font-mono text-zinc-300">{vpc.cidrBlock}</td>
                                <td class="px-4 py-3">
                                    <span class="px-2 py-0.5 rounded text-xs {vpc.state === 'available' ? 'bg-green-900/40 text-green-400' : 'bg-zinc-800 text-zinc-400'}">{vpc.state}</span>
                                </td>
                                <td class="px-4 py-3 text-zinc-400 text-xs">{vpc.isDefault === 'true' ? 'Yes' : 'No'}</td>
                                <td class="px-4 py-3">
                                    {#if confirmDeleteVpc === vpc.vpcId}
                                        <div class="flex items-center gap-1">
                                            <button onclick={() => handleDeleteVpc(vpc.vpcId)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteVpc = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button onclick={() => confirmDeleteVpc = vpc.vpcId} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                    {/if}
                                </td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}

    <!-- Subnets tab -->
    {#if activeTab === 'subnets'}
        {#if subnetsLoading}
            <div class="text-zinc-500">Loading subnets...</div>
        {:else if subnetsError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{subnetsError}</div>
        {:else if subnets.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No subnets found.</p>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Subnet ID</th>
                            <th class="px-4 py-3">VPC ID</th>
                            <th class="px-4 py-3">CIDR Block</th>
                            <th class="px-4 py-3">AZ</th>
                            <th class="px-4 py-3">Available IPs</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each subnets as subnet}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400 text-xs">{subnet.subnetId}</td>
                                <td class="px-4 py-3 font-mono text-zinc-400 text-xs">{subnet.vpcId}</td>
                                <td class="px-4 py-3 font-mono text-zinc-300">{subnet.cidrBlock}</td>
                                <td class="px-4 py-3 text-zinc-400">{subnet.availabilityZone}</td>
                                <td class="px-4 py-3 text-zinc-400">{subnet.availableIpAddressCount}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}

    <!-- Security Groups tab -->
    {#if activeTab === 'securityGroups'}
        {#if showCreateSg}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create Security Group</h3>
                {#if createSgError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createSgError}</div>
                {/if}
                <div class="grid grid-cols-3 gap-3 mb-3">
                    <div>
                        <label for="sg-name" class="block text-xs text-zinc-400 mb-1">Name</label>
                        <input id="sg-name" type="text" bind:value={newSgName} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" placeholder="my-sg" />
                    </div>
                    <div>
                        <label for="sg-desc" class="block text-xs text-zinc-400 mb-1">Description</label>
                        <input id="sg-desc" type="text" bind:value={newSgDesc} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" placeholder="My security group" />
                    </div>
                    <div>
                        <label for="sg-vpc" class="block text-xs text-zinc-400 mb-1">VPC ID</label>
                        <input id="sg-vpc" type="text" bind:value={newSgVpcId} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" placeholder="vpc-..." />
                    </div>
                </div>
                <div class="flex gap-2">
                    <button
                        onclick={handleCreateSg}
                        disabled={creatingSg || !newSgName.trim() || !newSgDesc.trim() || !newSgVpcId.trim()}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                    >
                        {creatingSg ? 'Creating...' : 'Create'}
                    </button>
                    <button
                        onclick={() => { showCreateSg = false; createSgError = null; newSgName = ''; newSgDesc = ''; newSgVpcId = ''; }}
                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        {/if}

        {#if sgLoading}
            <div class="text-zinc-500">Loading security groups...</div>
        {:else if sgError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{sgError}</div>
        {:else if securityGroups.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No security groups found.</p>
                <button onclick={() => showCreateSg = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                    Create your first security group
                </button>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Group ID</th>
                            <th class="px-4 py-3">Name</th>
                            <th class="px-4 py-3">Description</th>
                            <th class="px-4 py-3">VPC ID</th>
                            <th class="px-4 py-3"></th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each securityGroups as sg}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400 text-xs">{sg.groupId}</td>
                                <td class="px-4 py-3 text-zinc-300">{sg.groupName}</td>
                                <td class="px-4 py-3 text-zinc-400 text-xs">{sg.description}</td>
                                <td class="px-4 py-3 font-mono text-zinc-400 text-xs">{sg.vpcId}</td>
                                <td class="px-4 py-3">
                                    {#if confirmDeleteSg === sg.groupId}
                                        <div class="flex items-center gap-1">
                                            <button onclick={() => handleDeleteSg(sg.groupId)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteSg = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button onclick={() => confirmDeleteSg = sg.groupId} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
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
