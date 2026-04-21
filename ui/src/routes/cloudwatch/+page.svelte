<script lang="ts">
    import { onMount } from 'svelte';
    import { listLogGroups, type LogGroup } from '$lib/aws';

    let logGroups = $state<LogGroup[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    onMount(async () => {
        try {
            const data = await listLogGroups();
            logGroups = data.logGroups;
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
            <h1 class="text-2xl font-bold">CloudWatch — Log Groups</h1>
            <p class="text-zinc-500 mt-1">Monitor resources and applications. View metrics, logs, and alarms.</p>
        </div>
        <span class="text-sm text-zinc-500">{logGroups.length} log group{logGroups.length !== 1 ? 's' : ''}</span>
    </div>

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if logGroups.length === 0}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No log groups yet. Create one using the AWS CLI:</p>
            <code class="block mt-3 text-sm text-orange-400 font-mono">
                aws --endpoint-url http://localhost:4566 logs create-log-group --log-group-name /my/app
            </code>
        </div>
    {:else}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-zinc-800 text-left text-zinc-500">
                        <th class="px-4 py-3">Log Group Name</th>
                        <th class="px-4 py-3">Retention</th>
                        <th class="px-4 py-3">Stored Bytes</th>
                    </tr>
                </thead>
                <tbody>
                    {#each logGroups as group}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400">{group.name}</td>
                            <td class="px-4 py-3 text-zinc-400">
                                {group.retentionDays != null ? `${group.retentionDays} days` : 'Never expire'}
                            </td>
                            <td class="px-4 py-3 text-zinc-400">{group.storedBytes.toLocaleString()} B</td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    {/if}
</div>
