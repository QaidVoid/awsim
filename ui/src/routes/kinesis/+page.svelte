<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listStreams, createStream, deleteStream, describeStream,
        type KinesisStream, type KinesisShard,
    } from '$lib/aws';

    let streamNames = $state<string[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let showCreateForm = $state(false);
    let newStreamName = $state('');
    let newShardCount = $state(1);
    let creating = $state(false);
    let createError = $state<string | null>(null);
    let confirmDeleteStream = $state<string | null>(null);

    let selectedStream = $state<string | null>(null);
    let streamDetail = $state<KinesisStream | null>(null);
    let shards = $state<KinesisShard[]>([]);
    let detailLoading = $state(false);

    async function loadStreams() {
        loading = true;
        error = null;
        try {
            const data = await listStreams();
            streamNames = data.streamNames;
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to load streams';
        } finally {
            loading = false;
        }
    }

    async function handleCreateStream() {
        if (!newStreamName.trim()) return;
        creating = true;
        createError = null;
        try {
            await createStream(newStreamName.trim(), newShardCount);
            newStreamName = '';
            newShardCount = 1;
            showCreateForm = false;
            await loadStreams();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed to create stream';
        } finally {
            creating = false;
        }
    }

    async function handleDeleteStream(name: string) {
        try {
            await deleteStream(name);
            confirmDeleteStream = null;
            if (selectedStream === name) {
                selectedStream = null;
                streamDetail = null;
                shards = [];
            }
            await loadStreams();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete stream';
        }
    }

    async function selectStream(name: string) {
        selectedStream = name;
        detailLoading = true;
        streamDetail = null;
        shards = [];
        try {
            const data = await describeStream(name);
            streamDetail = data.stream;
            shards = data.shards;
        } catch {
            // silently fail detail load
        } finally {
            detailLoading = false;
        }
    }

    function statusColor(status: string): string {
        if (status === 'ACTIVE') return 'bg-green-900/40 text-green-400';
        if (status === 'CREATING' || status === 'UPDATING') return 'bg-yellow-900/40 text-yellow-400';
        if (status === 'DELETING') return 'bg-red-900/40 text-red-400';
        return 'bg-zinc-800 text-zinc-400';
    }

    onMount(() => loadStreams());
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">Kinesis — Data Streams</h1>
            <p class="text-zinc-500 mt-1">Real-time data streaming. Collect, process, and analyze streaming data.</p>
        </div>
        <div class="flex items-center gap-3">
            <span class="text-sm text-zinc-500">{streamNames.length} stream{streamNames.length !== 1 ? 's' : ''}</span>
            <button
                onclick={() => { showCreateForm = !showCreateForm; createError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Stream
            </button>
        </div>
    </div>

    {#if showCreateForm}
        <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
            <h3 class="font-semibold mb-3">Create Stream</h3>
            {#if createError}
                <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createError}</div>
            {/if}
            <div class="grid grid-cols-2 gap-3 mb-3">
                <div>
                    <label for="stream-name" class="block text-xs text-zinc-400 mb-1">Stream Name</label>
                    <input
                        id="stream-name"
                        type="text"
                        bind:value={newStreamName}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        placeholder="my-stream"
                    />
                </div>
                <div>
                    <label for="shard-count" class="block text-xs text-zinc-400 mb-1">Shard Count</label>
                    <input
                        id="shard-count"
                        type="number"
                        bind:value={newShardCount}
                        min="1"
                        max="100"
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                    />
                </div>
            </div>
            <div class="flex gap-2">
                <button
                    onclick={handleCreateStream}
                    disabled={creating || !newStreamName.trim()}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                >
                    {creating ? 'Creating...' : 'Create'}
                </button>
                <button
                    onclick={() => { showCreateForm = false; createError = null; newStreamName = ''; newShardCount = 1; }}
                    class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                >
                    Cancel
                </button>
            </div>
        </div>
    {/if}

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if streamNames.length === 0 && !showCreateForm}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No Kinesis streams yet.</p>
            <button onclick={() => showCreateForm = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                Create your first stream
            </button>
        </div>
    {:else}
        <div class="flex gap-4">
            <!-- Stream list -->
            <div class="w-72 shrink-0">
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                    {#each streamNames as name}
                        <div class="border-b border-zinc-800/50 last:border-0 {selectedStream === name ? 'bg-zinc-800' : 'hover:bg-zinc-800/40'} transition-colors">
                            <div class="px-4 py-3 flex items-start justify-between gap-2">
                                <button class="flex-1 text-left min-w-0" onclick={() => selectStream(name)}>
                                    <div class="font-mono text-orange-400 text-sm truncate">{name}</div>
                                </button>
                                <div class="flex items-center gap-1 shrink-0">
                                    {#if confirmDeleteStream === name}
                                        <button onclick={() => handleDeleteStream(name)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                        <button onclick={() => confirmDeleteStream = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                    {:else}
                                        <button onclick={(e) => { e.stopPropagation(); confirmDeleteStream = name; }} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                    {/if}
                                </div>
                            </div>
                        </div>
                    {/each}
                </div>
            </div>

            <!-- Stream detail -->
            <div class="flex-1 min-w-0">
                {#if selectedStream}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                        <div class="px-4 py-3 border-b border-zinc-800">
                            <span class="font-mono text-orange-400">{selectedStream}</span>
                        </div>

                        {#if detailLoading}
                            <div class="p-4 text-zinc-500 text-sm">Loading stream details...</div>
                        {:else if streamDetail}
                            <div class="p-4 grid grid-cols-3 gap-4 border-b border-zinc-800">
                                <div>
                                    <div class="text-xs text-zinc-500 mb-1">Status</div>
                                    <span class="px-2 py-0.5 rounded text-xs {statusColor(streamDetail.streamStatus)}">{streamDetail.streamStatus}</span>
                                </div>
                                <div>
                                    <div class="text-xs text-zinc-500 mb-1">Shards</div>
                                    <div class="text-sm text-zinc-300">{streamDetail.shardCount}</div>
                                </div>
                                <div>
                                    <div class="text-xs text-zinc-500 mb-1">Retention</div>
                                    <div class="text-sm text-zinc-300">{streamDetail.retentionPeriodHours}h</div>
                                </div>
                            </div>

                            <div class="px-4 py-3">
                                <h3 class="text-sm font-medium text-zinc-400 mb-3">Shards</h3>
                                {#if shards.length === 0}
                                    <p class="text-zinc-500 text-sm">No shards found.</p>
                                {:else}
                                    <table class="w-full text-sm">
                                        <thead>
                                            <tr class="text-left text-zinc-500 border-b border-zinc-800">
                                                <th class="pb-2 pr-4">Shard ID</th>
                                                <th class="pb-2 pr-4">Starting Sequence</th>
                                                <th class="pb-2">Ending Sequence</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {#each shards as shard}
                                                <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                                    <td class="py-2 pr-4 font-mono text-orange-400 text-xs">{shard.shardId}</td>
                                                    <td class="py-2 pr-4 text-zinc-400 text-xs font-mono">{shard.startingSequenceNumber}</td>
                                                    <td class="py-2 text-zinc-500 text-xs font-mono">{shard.endingSequenceNumber ?? '—'}</td>
                                                </tr>
                                            {/each}
                                        </tbody>
                                    </table>
                                {/if}
                            </div>
                        {:else}
                            <div class="p-4 text-zinc-500 text-sm">Could not load stream details.</div>
                        {/if}
                    </div>
                {:else}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-500 text-sm">
                        Select a stream to view shard details.
                    </div>
                {/if}
            </div>
        </div>
    {/if}
</div>
