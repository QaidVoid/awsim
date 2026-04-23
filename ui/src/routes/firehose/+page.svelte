<script lang="ts">
    import { onMount } from 'svelte';

    const BASE = 'http://localhost:4566';

    interface StreamInfo { name: string; status: string; destination: string; arn: string; }

    let streams = $state<StreamInfo[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let showCreate = $state(false);
    let newName = $state('');
    let newBucket = $state('arn:aws:s3:::my-bucket');
    let newRole = $state('arn:aws:iam::000000000000:role/firehose');
    let creating = $state(false);
    let createError = $state<string | null>(null);

    async function apiFetch(target: string, body: unknown) {
        const res = await fetch(`${BASE}/`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/x-amz-json-1.1',
                'X-Amz-Target': `Firehose_20150804.${target}`,
                'Authorization': 'AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/firehose/aws4_request, SignedHeaders=host, Signature=fake',
            },
            body: JSON.stringify(body ?? {}),
        });
        if (!res.ok) throw new Error(await res.text() || `HTTP ${res.status}`);
        const text = await res.text();
        return text ? JSON.parse(text) : {};
    }

    async function loadStreams() {
        loading = true;
        error = null;
        try {
            const list = await apiFetch('ListDeliveryStreams', {});
            const names: string[] = list.DeliveryStreamNames ?? [];
            const out: StreamInfo[] = [];
            for (const n of names) {
                try {
                    const d = await apiFetch('DescribeDeliveryStream', { DeliveryStreamName: n });
                    const desc = d.DeliveryStreamDescription;
                    const firstDest = desc?.Destinations?.[0] ?? {};
                    const destType = Object.keys(firstDest).find((k) => k.endsWith('DestinationDescription')) ?? '-';
                    out.push({
                        name: desc?.DeliveryStreamName ?? n,
                        status: desc?.DeliveryStreamStatus ?? '',
                        arn: desc?.DeliveryStreamARN ?? '',
                        destination: destType.replace('DestinationDescription', '') || '-',
                    });
                } catch {
                    out.push({ name: n, status: 'UNKNOWN', destination: '-', arn: '' });
                }
            }
            streams = out;
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed';
        } finally {
            loading = false;
        }
    }

    async function createStream() {
        if (!newName.trim()) return;
        creating = true;
        createError = null;
        try {
            await apiFetch('CreateDeliveryStream', {
                DeliveryStreamName: newName.trim(),
                DeliveryStreamType: 'DirectPut',
                S3DestinationConfiguration: {
                    BucketARN: newBucket.trim(),
                    RoleARN: newRole.trim(),
                    BufferingHints: { SizeInMBs: 5, IntervalInSeconds: 300 },
                    CompressionFormat: 'UNCOMPRESSED',
                },
            });
            newName = '';
            showCreate = false;
            await loadStreams();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed';
        } finally {
            creating = false;
        }
    }

    async function deleteStream(name: string) {
        if (!confirm(`Delete stream ${name}?`)) return;
        try {
            await apiFetch('DeleteDeliveryStream', { DeliveryStreamName: name });
            await loadStreams();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed';
        }
    }

    function statusColor(s: string): string {
        if (s === 'ACTIVE') return 'bg-green-900/40 text-green-300';
        if (s === 'CREATING') return 'bg-yellow-900/40 text-yellow-300';
        if (s === 'DELETING') return 'bg-red-900/40 text-red-300';
        return 'bg-zinc-800 text-zinc-400';
    }

    onMount(loadStreams);
</script>

<div class="p-6 max-w-6xl mx-auto">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">Firehose</h1>
            <p class="text-zinc-500 mt-1">Data delivery streams.</p>
        </div>
        <button onclick={() => { showCreate = !showCreate; createError = null; }} class="px-4 py-2 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">Create Delivery Stream</button>
    </div>

    {#if showCreate}
        <div class="mb-6 p-4 bg-zinc-900 border border-zinc-700 rounded-lg">
            {#if createError}<p class="text-red-400 text-xs mb-2">{createError}</p>{/if}
            <div class="space-y-3">
                <input type="text" bind:value={newName} placeholder="Stream name" class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm focus:outline-none focus:border-orange-500" />
                <input type="text" bind:value={newBucket} placeholder="S3 Bucket ARN" class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm focus:outline-none focus:border-orange-500" />
                <input type="text" bind:value={newRole} placeholder="Role ARN" class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm focus:outline-none focus:border-orange-500" />
                <div class="flex gap-2">
                    <button onclick={createStream} disabled={creating} class="px-4 py-2 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm">{creating ? 'Creating...' : 'Create'}</button>
                    <button onclick={() => showCreate = false} class="px-4 py-2 bg-zinc-700 hover:bg-zinc-600 rounded text-sm">Cancel</button>
                </div>
            </div>
        </div>
    {/if}

    {#if loading}
        <p class="text-zinc-500 text-sm">Loading...</p>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if streams.length === 0}
        <div class="text-center py-16 text-zinc-500">No delivery streams.</div>
    {:else}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-zinc-800 text-left text-zinc-500">
                        <th class="px-4 py-3 text-xs">Name</th>
                        <th class="px-4 py-3 text-xs">Status</th>
                        <th class="px-4 py-3 text-xs">Destination</th>
                        <th class="px-4 py-3 text-xs">ARN</th>
                        <th class="px-4 py-3 text-xs text-right"></th>
                    </tr>
                </thead>
                <tbody>
                    {#each streams as s}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 text-zinc-200">{s.name}</td>
                            <td class="px-4 py-3"><span class="px-1.5 py-0.5 rounded text-xs {statusColor(s.status)}">{s.status}</span></td>
                            <td class="px-4 py-3 text-zinc-400 text-xs">{s.destination}</td>
                            <td class="px-4 py-3 font-mono text-orange-400 text-xs truncate max-w-xs">{s.arn}</td>
                            <td class="px-4 py-3 text-right"><button onclick={() => deleteStream(s.name)} class="text-red-400 hover:text-red-300 text-xs">Delete</button></td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    {/if}
</div>
