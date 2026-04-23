<script lang="ts">
    import { onMount } from 'svelte';

    const BASE = 'http://localhost:4566';

    interface Account { Id: string; Arn: string; Email: string; Name: string; Status: string; }
    interface OU { Id: string; Arn: string; Name: string; }
    interface Policy { Id: string; Arn: string; Name: string; Type: string; Description?: string; AwsManaged?: boolean; }
    interface Root { Id: string; Arn: string; Name: string; }

    let activeTab = $state<'accounts' | 'ous' | 'policies' | 'roots'>('accounts');
    let accounts = $state<Account[]>([]);
    let roots = $state<Root[]>([]);
    let ous = $state<OU[]>([]);
    let policies = $state<Policy[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    async function apiFetch(target: string, body: unknown) {
        const res = await fetch(`${BASE}/`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/x-amz-json-1.1',
                'X-Amz-Target': `AWSOrganizationsV20161128.${target}`,
                'Authorization': 'AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/organizations/aws4_request, SignedHeaders=host, Signature=fake',
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
            const [a, r, p] = await Promise.all([
                apiFetch('ListAccounts', {}),
                apiFetch('ListRoots', {}),
                apiFetch('ListPolicies', { Filter: 'SERVICE_CONTROL_POLICY' }),
            ]);
            accounts = a.Accounts ?? [];
            roots = r.Roots ?? [];
            policies = p.Policies ?? [];

            const allOus: OU[] = [];
            for (const root of roots) {
                try {
                    const o = await apiFetch('ListOrganizationalUnitsForParent', { ParentId: root.Id });
                    allOus.push(...(o.OrganizationalUnits ?? []));
                } catch { /* skip */ }
            }
            ous = allOus;
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed';
        } finally {
            loading = false;
        }
    }

    function statusColor(s: string): string {
        if (s === 'ACTIVE') return 'bg-green-900/40 text-green-300';
        if (s === 'SUSPENDED') return 'bg-red-900/40 text-red-300';
        return 'bg-zinc-800 text-zinc-400';
    }

    onMount(loadAll);
</script>

<div class="p-6">
    <div class="mb-6">
        <h1 class="text-2xl font-bold">Organizations</h1>
        <p class="text-zinc-500 mt-1">Accounts, OUs, policies, and roots.</p>
    </div>

    <div class="flex gap-1 mb-6 border-b border-zinc-800">
        <button onclick={() => activeTab = 'accounts'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'accounts' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Accounts ({accounts.length})</button>
        <button onclick={() => activeTab = 'ous'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'ous' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">OUs ({ous.length})</button>
        <button onclick={() => activeTab = 'policies'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'policies' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Policies (SCPs) ({policies.length})</button>
        <button onclick={() => activeTab = 'roots'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'roots' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Roots ({roots.length})</button>
    </div>

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if activeTab === 'accounts'}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">ID</th><th class="px-4 py-3 text-xs">Name</th><th class="px-4 py-3 text-xs">Email</th><th class="px-4 py-3 text-xs">Status</th></tr></thead>
                <tbody>
                    {#each accounts as a}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400 text-xs">{a.Id}</td>
                            <td class="px-4 py-3 text-zinc-200">{a.Name}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs">{a.Email}</td>
                            <td class="px-4 py-3"><span class="px-1.5 py-0.5 rounded text-xs {statusColor(a.Status)}">{a.Status}</span></td>
                        </tr>
                    {/each}
                    {#if accounts.length === 0}<tr><td colspan="4" class="px-4 py-8 text-center text-zinc-500">No accounts.</td></tr>{/if}
                </tbody>
            </table>
        </div>
    {:else if activeTab === 'ous'}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">ID</th><th class="px-4 py-3 text-xs">Name</th><th class="px-4 py-3 text-xs">ARN</th></tr></thead>
                <tbody>
                    {#each ous as o}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400 text-xs">{o.Id}</td>
                            <td class="px-4 py-3 text-zinc-200">{o.Name}</td>
                            <td class="px-4 py-3 font-mono text-zinc-500 text-xs truncate max-w-xs">{o.Arn}</td>
                        </tr>
                    {/each}
                    {#if ous.length === 0}<tr><td colspan="3" class="px-4 py-8 text-center text-zinc-500">No OUs.</td></tr>{/if}
                </tbody>
            </table>
        </div>
    {:else if activeTab === 'policies'}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">ID</th><th class="px-4 py-3 text-xs">Name</th><th class="px-4 py-3 text-xs">Type</th><th class="px-4 py-3 text-xs">Description</th></tr></thead>
                <tbody>
                    {#each policies as p}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400 text-xs">{p.Id}</td>
                            <td class="px-4 py-3 text-zinc-200">{p.Name}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs">{p.Type}</td>
                            <td class="px-4 py-3 text-zinc-500 text-xs">{p.Description ?? ''}</td>
                        </tr>
                    {/each}
                    {#if policies.length === 0}<tr><td colspan="4" class="px-4 py-8 text-center text-zinc-500">No policies.</td></tr>{/if}
                </tbody>
            </table>
        </div>
    {:else}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">ID</th><th class="px-4 py-3 text-xs">Name</th><th class="px-4 py-3 text-xs">ARN</th></tr></thead>
                <tbody>
                    {#each roots as r}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400 text-xs">{r.Id}</td>
                            <td class="px-4 py-3 text-zinc-200">{r.Name}</td>
                            <td class="px-4 py-3 font-mono text-zinc-500 text-xs truncate max-w-xs">{r.Arn}</td>
                        </tr>
                    {/each}
                    {#if roots.length === 0}<tr><td colspan="3" class="px-4 py-8 text-center text-zinc-500">No roots.</td></tr>{/if}
                </tbody>
            </table>
        </div>
    {/if}
</div>
