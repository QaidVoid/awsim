<script lang="ts">
    import { onMount } from 'svelte';
    import { listFunctions, type LambdaFunction } from '$lib/aws';

    let functions = $state<LambdaFunction[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    onMount(async () => {
        try {
            const data = await listFunctions();
            functions = data.functions;
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
            <h1 class="text-2xl font-bold">Lambda — Functions</h1>
            <p class="text-zinc-500 mt-1">Serverless compute. Run code without provisioning servers.</p>
        </div>
        <span class="text-sm text-zinc-500">{functions.length} function{functions.length !== 1 ? 's' : ''}</span>
    </div>

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if functions.length === 0}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No functions yet. Create one using the AWS CLI:</p>
            <code class="block mt-3 text-sm text-orange-400 font-mono">
                aws --endpoint-url http://localhost:4566 lambda create-function --function-name my-fn --runtime nodejs20.x --role arn:aws:iam::000000000000:role/exec --handler index.handler --zip-file fileb://function.zip
            </code>
        </div>
    {:else}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-zinc-800 text-left text-zinc-500">
                        <th class="px-4 py-3">Function Name</th>
                        <th class="px-4 py-3">Runtime</th>
                        <th class="px-4 py-3">Memory</th>
                        <th class="px-4 py-3">Handler</th>
                        <th class="px-4 py-3">Last Modified</th>
                    </tr>
                </thead>
                <tbody>
                    {#each functions as fn}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400">{fn.name}</td>
                            <td class="px-4 py-3 text-zinc-300">{fn.runtime}</td>
                            <td class="px-4 py-3 text-zinc-400">{fn.memory} MB</td>
                            <td class="px-4 py-3 font-mono text-zinc-400 text-xs">{fn.handler}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs">{fn.lastModified}</td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    {/if}
</div>
