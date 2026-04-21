<script lang="ts">
    import { onMount } from 'svelte';

    type Scope = 'REGIONAL' | 'CLOUDFRONT';

    interface WebAclSummary {
        ARN: string;
        Id: string;
        Name: string;
        LockToken: string;
    }

    interface IpSetSummary {
        ARN: string;
        Id: string;
        Name: string;
        LockToken: string;
    }

    let activeTab = $state<'webacls' | 'ipsets'>('webacls');
    let scope = $state<Scope>('REGIONAL');

    // WebACLs
    let webAcls = $state<WebAclSummary[]>([]);
    let webAclsLoading = $state(false);
    let webAclsError = $state<string | null>(null);
    let showCreateAcl = $state(false);
    let newAclName = $state('');
    let newAclAction = $state<'Allow' | 'Block'>('Allow');
    let creatingAcl = $state(false);

    // IP Sets
    let ipSets = $state<IpSetSummary[]>([]);
    let ipSetsLoading = $state(false);
    let ipSetsError = $state<string | null>(null);
    let showCreateIpSet = $state(false);
    let newIpSetName = $state('');
    let newIpAddresses = $state('');
    let newIpVersion = $state<'IPV4' | 'IPV6'>('IPV4');
    let creatingIpSet = $state(false);

    async function wafPost(operation: string, body: Record<string, unknown>) {
        const res = await fetch('http://localhost:4566/', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/x-amz-json-1.1',
                'X-Amz-Target': `AWSWAF_20190729.${operation}`,
                'X-Amz-Security-Token': 'local',
            },
            body: JSON.stringify(body),
        });
        const data = await res.json();
        if (!res.ok) throw new Error(data.message ?? res.statusText);
        return data;
    }

    async function loadWebAcls() {
        webAclsLoading = true;
        webAclsError = null;
        try {
            const data = await wafPost('ListWebACLs', { Scope: scope });
            webAcls = data.WebACLs ?? [];
        } catch (e: any) {
            webAclsError = e.message;
        } finally {
            webAclsLoading = false;
        }
    }

    async function createWebAcl() {
        if (!newAclName.trim()) return;
        creatingAcl = true;
        try {
            await wafPost('CreateWebACL', {
                Name: newAclName.trim(),
                Scope: scope,
                DefaultAction: newAclAction === 'Allow' ? { Allow: {} } : { Block: {} },
                Rules: [],
                VisibilityConfig: {
                    SampledRequestsEnabled: true,
                    CloudWatchMetricsEnabled: true,
                    MetricName: newAclName.trim(),
                },
            });
            newAclName = '';
            showCreateAcl = false;
            await loadWebAcls();
        } catch (e: any) {
            alert(e.message);
        } finally {
            creatingAcl = false;
        }
    }

    async function deleteWebAcl(acl: WebAclSummary) {
        if (!confirm(`Delete WebACL "${acl.Name}"?`)) return;
        await wafPost('DeleteWebACL', { Name: acl.Name, Scope: scope, Id: acl.Id, LockToken: acl.LockToken });
        await loadWebAcls();
    }

    async function loadIpSets() {
        ipSetsLoading = true;
        ipSetsError = null;
        try {
            const data = await wafPost('ListIPSets', { Scope: scope });
            ipSets = data.IPSets ?? [];
        } catch (e: any) {
            ipSetsError = e.message;
        } finally {
            ipSetsLoading = false;
        }
    }

    async function createIpSet() {
        if (!newIpSetName.trim()) return;
        creatingIpSet = true;
        try {
            const addresses = newIpAddresses.split(',').map(s => s.trim()).filter(Boolean);
            await wafPost('CreateIPSet', {
                Name: newIpSetName.trim(),
                Scope: scope,
                IPAddressVersion: newIpVersion,
                Addresses: addresses,
            });
            newIpSetName = '';
            newIpAddresses = '';
            showCreateIpSet = false;
            await loadIpSets();
        } catch (e: any) {
            alert(e.message);
        } finally {
            creatingIpSet = false;
        }
    }

    async function deleteIpSet(s: IpSetSummary) {
        if (!confirm(`Delete IP set "${s.Name}"?`)) return;
        await wafPost('DeleteIPSet', { Name: s.Name, Scope: scope, Id: s.Id, LockToken: s.LockToken });
        await loadIpSets();
    }

    function switchTab(tab: 'webacls' | 'ipsets') {
        activeTab = tab;
        if (tab === 'webacls') loadWebAcls();
        else loadIpSets();
    }

    onMount(() => {
        loadWebAcls();
    });
</script>

<div class="p-6 max-w-5xl mx-auto">
    <div class="flex items-center justify-between mb-4">
        <div>
            <h1 class="text-2xl font-bold text-zinc-100">WAF v2</h1>
            <p class="text-zinc-400 text-sm mt-1">Web Application Firewall</p>
        </div>
        <div class="flex items-center gap-2">
            <label class="text-xs text-zinc-400">Scope:</label>
            <select
                bind:value={scope}
                onchange={() => (activeTab === 'webacls' ? loadWebAcls() : loadIpSets())}
                class="px-2 py-1.5 bg-zinc-800 border border-zinc-700 rounded text-sm text-zinc-100"
            >
                <option value="REGIONAL">REGIONAL</option>
                <option value="CLOUDFRONT">CLOUDFRONT</option>
            </select>
        </div>
    </div>

    <!-- Tabs -->
    <div class="flex gap-1 mb-6 border-b border-zinc-800">
        {#each [['webacls', 'Web ACLs'], ['ipsets', 'IP Sets']] as [id, label]}
            <button
                onclick={() => switchTab(id as 'webacls' | 'ipsets')}
                class="px-4 py-2 text-sm font-medium transition-colors {activeTab === id
                    ? 'text-orange-400 border-b-2 border-orange-400'
                    : 'text-zinc-400 hover:text-zinc-200'}"
            >
                {label}
            </button>
        {/each}
    </div>

    {#if activeTab === 'webacls'}
        <div class="flex justify-between items-center mb-4">
            <h2 class="text-sm font-semibold text-zinc-300">Web ACLs</h2>
            <button
                onclick={() => (showCreateAcl = !showCreateAcl)}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 text-white rounded text-xs font-medium"
            >
                Create WebACL
            </button>
        </div>

        {#if showCreateAcl}
            <div class="mb-4 p-4 bg-zinc-900 border border-zinc-700 rounded-lg space-y-3">
                <div>
                    <label class="block text-xs text-zinc-400 mb-1">Name</label>
                    <input
                        type="text"
                        bind:value={newAclName}
                        placeholder="my-web-acl"
                        class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm text-zinc-100 focus:outline-none focus:border-orange-500"
                    />
                </div>
                <div>
                    <label class="block text-xs text-zinc-400 mb-1">Default Action</label>
                    <select bind:value={newAclAction} class="px-2 py-1.5 bg-zinc-800 border border-zinc-700 rounded text-sm text-zinc-100">
                        <option value="Allow">Allow</option>
                        <option value="Block">Block</option>
                    </select>
                </div>
                <div class="flex gap-2">
                    <button onclick={createWebAcl} disabled={creatingAcl || !newAclName.trim()} class="px-4 py-2 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 text-white rounded text-sm">
                        {creatingAcl ? 'Creating...' : 'Create'}
                    </button>
                    <button onclick={() => (showCreateAcl = false)} class="px-4 py-2 bg-zinc-700 hover:bg-zinc-600 text-zinc-200 rounded text-sm">Cancel</button>
                </div>
            </div>
        {/if}

        {#if webAclsLoading}
            <p class="text-zinc-400 text-sm">Loading...</p>
        {:else if webAclsError}
            <p class="text-red-400 text-sm">{webAclsError}</p>
        {:else if webAcls.length === 0}
            <div class="text-center py-12 text-zinc-500 text-sm">No Web ACLs in {scope} scope.</div>
        {:else}
            <div class="space-y-2">
                {#each webAcls as acl}
                    <div class="flex items-center justify-between p-4 bg-zinc-900 border border-zinc-800 rounded-lg">
                        <div>
                            <p class="text-sm font-medium text-zinc-100">{acl.Name}</p>
                            <p class="text-xs text-zinc-500 font-mono mt-0.5">{acl.ARN}</p>
                        </div>
                        <button onclick={() => deleteWebAcl(acl)} class="px-3 py-1.5 text-xs bg-red-900 hover:bg-red-800 text-red-200 rounded">Delete</button>
                    </div>
                {/each}
            </div>
        {/if}
    {:else}
        <div class="flex justify-between items-center mb-4">
            <h2 class="text-sm font-semibold text-zinc-300">IP Sets</h2>
            <button onclick={() => (showCreateIpSet = !showCreateIpSet)} class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 text-white rounded text-xs font-medium">
                Create IP Set
            </button>
        </div>

        {#if showCreateIpSet}
            <div class="mb-4 p-4 bg-zinc-900 border border-zinc-700 rounded-lg space-y-3">
                <div>
                    <label class="block text-xs text-zinc-400 mb-1">Name</label>
                    <input type="text" bind:value={newIpSetName} placeholder="my-ip-set" class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm text-zinc-100 focus:outline-none focus:border-orange-500" />
                </div>
                <div>
                    <label class="block text-xs text-zinc-400 mb-1">IP Version</label>
                    <select bind:value={newIpVersion} class="px-2 py-1.5 bg-zinc-800 border border-zinc-700 rounded text-sm text-zinc-100">
                        <option value="IPV4">IPV4</option>
                        <option value="IPV6">IPV6</option>
                    </select>
                </div>
                <div>
                    <label class="block text-xs text-zinc-400 mb-1">Addresses (CIDR, comma-separated)</label>
                    <input type="text" bind:value={newIpAddresses} placeholder="10.0.0.0/8, 192.168.0.0/16" class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm text-zinc-100 focus:outline-none focus:border-orange-500" />
                </div>
                <div class="flex gap-2">
                    <button onclick={createIpSet} disabled={creatingIpSet || !newIpSetName.trim()} class="px-4 py-2 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 text-white rounded text-sm">
                        {creatingIpSet ? 'Creating...' : 'Create'}
                    </button>
                    <button onclick={() => (showCreateIpSet = false)} class="px-4 py-2 bg-zinc-700 hover:bg-zinc-600 text-zinc-200 rounded text-sm">Cancel</button>
                </div>
            </div>
        {/if}

        {#if ipSetsLoading}
            <p class="text-zinc-400 text-sm">Loading...</p>
        {:else if ipSetsError}
            <p class="text-red-400 text-sm">{ipSetsError}</p>
        {:else if ipSets.length === 0}
            <div class="text-center py-12 text-zinc-500 text-sm">No IP sets in {scope} scope.</div>
        {:else}
            <div class="space-y-2">
                {#each ipSets as s}
                    <div class="flex items-center justify-between p-4 bg-zinc-900 border border-zinc-800 rounded-lg">
                        <div>
                            <p class="text-sm font-medium text-zinc-100">{s.Name}</p>
                            <p class="text-xs text-zinc-500 font-mono mt-0.5">{s.ARN}</p>
                        </div>
                        <button onclick={() => deleteIpSet(s)} class="px-3 py-1.5 text-xs bg-red-900 hover:bg-red-800 text-red-200 rounded">Delete</button>
                    </div>
                {/each}
            </div>
        {/if}
    {/if}
</div>
