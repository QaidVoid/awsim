<script lang="ts">
    import { onMount } from 'svelte';

    const BASE = 'http://localhost:4566';

    interface Instance { InstanceArn: string; IdentityStoreId: string; Name: string; Status: string; }
    interface Assignment { AccountId: string; PermissionSetArn: string; PrincipalId: string; PrincipalType: string; }
    interface PermissionSetDetail { Name: string; PermissionSetArn: string; Description?: string; SessionDuration?: string; }

    let activeTab = $state<'instances' | 'permissionSets' | 'assignments'>('instances');
    let instances = $state<Instance[]>([]);
    let permissionSets = $state<PermissionSetDetail[]>([]);
    let assignments = $state<Assignment[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    async function apiFetch(target: string, body: unknown) {
        const res = await fetch(`${BASE}/`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/x-amz-json-1.1',
                'X-Amz-Target': `SWBExternalService.${target}`,
                'Authorization': 'AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/sso/aws4_request, SignedHeaders=host, Signature=fake',
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
            const inst = await apiFetch('ListInstances', {});
            instances = inst.Instances ?? [];
            const first = instances[0];

            if (first) {
                const ps = await apiFetch('ListPermissionSets', { InstanceArn: first.InstanceArn });
                const psArns: string[] = ps.PermissionSets ?? [];
                const details: PermissionSetDetail[] = [];
                for (const arn of psArns) {
                    try {
                        const d = await apiFetch('DescribePermissionSet', { InstanceArn: first.InstanceArn, PermissionSetArn: arn });
                        if (d.PermissionSet) details.push(d.PermissionSet);
                    } catch { /* skip */ }
                }
                permissionSets = details;

                let allAssigns: Assignment[] = [];
                for (const d of details) {
                    try {
                        const r = await apiFetch('ListAccountAssignments', { InstanceArn: first.InstanceArn, PermissionSetArn: d.PermissionSetArn, AccountId: '000000000000' });
                        allAssigns = allAssigns.concat(r.AccountAssignments ?? []);
                    } catch { /* skip */ }
                }
                assignments = allAssigns;
            } else {
                permissionSets = [];
                assignments = [];
            }
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed';
        } finally {
            loading = false;
        }
    }

    async function deletePermissionSet(arn: string) {
        const inst = instances[0];
        if (!inst) return;
        if (!confirm('Delete permission set?')) return;
        try {
            await apiFetch('DeletePermissionSet', { InstanceArn: inst.InstanceArn, PermissionSetArn: arn });
            await loadAll();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed';
        }
    }

    onMount(loadAll);
</script>

<div class="p-6">
    <div class="mb-6">
        <h1 class="text-2xl font-bold">SSO Admin</h1>
        <p class="text-zinc-500 mt-1">IAM Identity Center instances, permission sets, assignments.</p>
    </div>

    <div class="flex gap-1 mb-6 border-b border-zinc-800">
        <button onclick={() => activeTab = 'instances'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'instances' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Instances ({instances.length})</button>
        <button onclick={() => activeTab = 'permissionSets'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'permissionSets' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Permission Sets ({permissionSets.length})</button>
        <button onclick={() => activeTab = 'assignments'} class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'assignments' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-400 hover:text-zinc-200'}">Account Assignments ({assignments.length})</button>
    </div>

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if activeTab === 'instances'}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">Instance ARN</th><th class="px-4 py-3 text-xs">Identity Store</th><th class="px-4 py-3 text-xs">Name</th><th class="px-4 py-3 text-xs">Status</th></tr></thead>
                <tbody>
                    {#each instances as i}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400 text-xs">{i.InstanceArn}</td>
                            <td class="px-4 py-3 font-mono text-zinc-400 text-xs">{i.IdentityStoreId}</td>
                            <td class="px-4 py-3 text-zinc-200">{i.Name}</td>
                            <td class="px-4 py-3"><span class="px-1.5 py-0.5 rounded text-xs bg-green-900/40 text-green-300">{i.Status}</span></td>
                        </tr>
                    {/each}
                    {#if instances.length === 0}<tr><td colspan="4" class="px-4 py-8 text-center text-zinc-500">No instances.</td></tr>{/if}
                </tbody>
            </table>
        </div>
    {:else if activeTab === 'permissionSets'}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">Name</th><th class="px-4 py-3 text-xs">ARN</th><th class="px-4 py-3 text-xs">Session</th><th class="px-4 py-3 text-xs"></th></tr></thead>
                <tbody>
                    {#each permissionSets as p}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 text-zinc-200">{p.Name}</td>
                            <td class="px-4 py-3 font-mono text-orange-400 text-xs truncate max-w-xs">{p.PermissionSetArn}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs">{p.SessionDuration ?? ''}</td>
                            <td class="px-4 py-3 text-right"><button onclick={() => deletePermissionSet(p.PermissionSetArn)} class="text-red-400 hover:text-red-300 text-xs">Delete</button></td>
                        </tr>
                    {/each}
                    {#if permissionSets.length === 0}<tr><td colspan="4" class="px-4 py-8 text-center text-zinc-500">No permission sets.</td></tr>{/if}
                </tbody>
            </table>
        </div>
    {:else}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">Account ID</th><th class="px-4 py-3 text-xs">Permission Set</th><th class="px-4 py-3 text-xs">Principal</th><th class="px-4 py-3 text-xs">Type</th></tr></thead>
                <tbody>
                    {#each assignments as a}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-zinc-400 text-xs">{a.AccountId}</td>
                            <td class="px-4 py-3 font-mono text-orange-400 text-xs truncate max-w-xs">{a.PermissionSetArn}</td>
                            <td class="px-4 py-3 font-mono text-zinc-300 text-xs">{a.PrincipalId}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs">{a.PrincipalType}</td>
                        </tr>
                    {/each}
                    {#if assignments.length === 0}<tr><td colspan="4" class="px-4 py-8 text-center text-zinc-500">No assignments.</td></tr>{/if}
                </tbody>
            </table>
        </div>
    {/if}
</div>
