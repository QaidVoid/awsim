<script lang="ts">
    import { onMount } from 'svelte';
    import { listStateMachines, type StateMachine } from '$lib/aws';

    let stateMachines = $state<StateMachine[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    onMount(async () => {
        try {
            const data = await listStateMachines();
            stateMachines = data.stateMachines;
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
            <h1 class="text-2xl font-bold">Step Functions — State Machines</h1>
            <p class="text-zinc-500 mt-1">Coordinate distributed applications using visual workflows.</p>
        </div>
        <span class="text-sm text-zinc-500">{stateMachines.length} state machine{stateMachines.length !== 1 ? 's' : ''}</span>
    </div>

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if stateMachines.length === 0}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No state machines yet. Create one using the AWS CLI:</p>
            <code class="block mt-3 text-sm text-orange-400 font-mono">
                aws --endpoint-url http://localhost:4566 stepfunctions create-state-machine --name my-machine --definition file://definition.json --role-arn arn:aws:iam::000000000000:role/exec
            </code>
        </div>
    {:else}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-zinc-800 text-left text-zinc-500">
                        <th class="px-4 py-3">Name</th>
                        <th class="px-4 py-3">Type</th>
                        <th class="px-4 py-3">ARN</th>
                        <th class="px-4 py-3">Created</th>
                    </tr>
                </thead>
                <tbody>
                    {#each stateMachines as machine}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400">{machine.name}</td>
                            <td class="px-4 py-3 text-zinc-300">{machine.type}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs font-mono">{machine.arn}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs">{machine.creationDate}</td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    {/if}
</div>
