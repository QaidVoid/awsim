<script lang="ts">
    import { onMount } from 'svelte';
    import { listQueues, type SqsQueue } from '$lib/aws';

    let queues = $state<SqsQueue[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    onMount(async () => {
        try {
            const data = await listQueues();
            queues = data.queues;
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
            <h1 class="text-2xl font-bold">SQS — Queues</h1>
            <p class="text-zinc-500 mt-1">Simple Queue Service. Send, receive, and manage messages.</p>
        </div>
        <span class="text-sm text-zinc-500">{queues.length} queue{queues.length !== 1 ? 's' : ''}</span>
    </div>

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if queues.length === 0}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No queues yet. Create one using the AWS CLI:</p>
            <code class="block mt-3 text-sm text-orange-400 font-mono">
                aws --endpoint-url http://localhost:4566 sqs create-queue --queue-name my-queue
            </code>
        </div>
    {:else}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-zinc-800 text-left text-zinc-500">
                        <th class="px-4 py-3">Queue Name</th>
                        <th class="px-4 py-3">URL</th>
                    </tr>
                </thead>
                <tbody>
                    {#each queues as queue}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400">{queue.name}</td>
                            <td class="px-4 py-3 text-zinc-400 text-xs font-mono break-all">{queue.url}</td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    {/if}
</div>
