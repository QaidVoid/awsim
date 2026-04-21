<script lang="ts">
    import { onMount } from 'svelte';

    const BASE = 'http://localhost:4566';

    interface WorkGroup {
        Name: string;
        State: string;
        Description?: string;
        CreationTime?: string;
    }

    interface QueryExecution {
        QueryExecutionId: string;
        Query: string;
        WorkGroup: string;
        Status?: { State: string };
    }

    let workgroups = $state<WorkGroup[]>([]);
    let queryIds = $state<string[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    async function athenaRequest(target: string, body: Record<string, unknown> = {}) {
        const res = await fetch(BASE, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/x-amz-json-1.1',
                'X-Amz-Target': `AmazonAthena.${target}`,
                'Authorization': 'AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/athena/aws4_request, SignedHeaders=host, Signature=fake',
            },
            body: JSON.stringify(body),
        });
        if (!res.ok) throw new Error(await res.text());
        return res.json();
    }

    onMount(async () => {
        try {
            const [wgData, qeData] = await Promise.all([
                athenaRequest('ListWorkGroups'),
                athenaRequest('ListQueryExecutions'),
            ]);
            workgroups = wgData.WorkGroups ?? [];
            queryIds = qeData.QueryExecutionIds ?? [];
        } catch {
            error = 'Could not connect to AWSim. Is it running on port 4566?';
        } finally {
            loading = false;
        }
    });
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">Athena</h1>
            <p class="text-zinc-500 mt-1">Interactive SQL query service for data in S3.</p>
        </div>
    </div>

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else}
        <!-- WorkGroups -->
        <h2 class="text-lg font-semibold mb-3">WorkGroups ({workgroups.length})</h2>
        {#if workgroups.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-6 text-center mb-6">
                <p class="text-zinc-500">No workgroups (besides primary). Create one using the AWS CLI:</p>
                <code class="block mt-3 text-sm text-orange-400 font-mono">
                    aws --endpoint-url http://localhost:4566 athena create-work-group --name my-wg
                </code>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden mb-6">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Name</th>
                            <th class="px-4 py-3">State</th>
                            <th class="px-4 py-3">Description</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each workgroups as wg}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400">{wg.Name}</td>
                                <td class="px-4 py-3 text-zinc-300">{wg.State}</td>
                                <td class="px-4 py-3 text-zinc-500">{wg.Description ?? ''}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}

        <!-- Query Executions -->
        <h2 class="text-lg font-semibold mb-3">Query Executions ({queryIds.length})</h2>
        {#if queryIds.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-6 text-center">
                <p class="text-zinc-500">No query executions yet. Run a query:</p>
                <code class="block mt-3 text-sm text-orange-400 font-mono">
                    aws --endpoint-url http://localhost:4566 athena start-query-execution --query-string "SELECT 1"
                </code>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Query Execution ID</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each queryIds as id}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-zinc-300 text-xs">{id}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}
</div>
