<script lang="ts">
    import { onMount } from 'svelte';

    const ENDPOINT = 'http://localhost:4566';

    interface HostedZone {
        id: string;
        name: string;
        callerReference: string;
        recordSetCount: number;
    }

    let zones = $state<HostedZone[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    async function fetchZones() {
        loading = true;
        error = null;
        try {
            const res = await fetch(`${ENDPOINT}/2013-04-01/hostedzone`, {
                headers: { 'Content-Type': 'application/xml' },
            });
            if (!res.ok) throw new Error(`HTTP ${res.status}`);
            const text = await res.text();
            // Parse simple XML — extract HostedZone elements
            const parser = new DOMParser();
            const doc = parser.parseFromString(text, 'application/xml');
            const zoneEls = doc.querySelectorAll('HostedZone');
            zones = Array.from(zoneEls).map((el) => ({
                id: el.querySelector('Id')?.textContent ?? '',
                name: el.querySelector('Name')?.textContent ?? '',
                callerReference: el.querySelector('CallerReference')?.textContent ?? '',
                recordSetCount: parseInt(el.querySelector('ResourceRecordSetCount')?.textContent ?? '0', 10),
            }));
        } catch (e) {
            error = 'Could not connect to AWSim. Is it running on port 4566?';
        } finally {
            loading = false;
        }
    }

    onMount(fetchZones);
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">Route53 — Hosted Zones</h1>
            <p class="text-zinc-500 mt-1">Manage DNS hosted zones and resource record sets.</p>
        </div>
        <span class="text-sm text-zinc-500">{zones.length} zone{zones.length !== 1 ? 's' : ''}</span>
    </div>

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if zones.length === 0}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No hosted zones yet. Create one using the AWS CLI:</p>
            <code class="block mt-3 text-sm text-orange-400 font-mono">
                aws --endpoint-url http://localhost:4566 route53 create-hosted-zone --name example.com --caller-reference ref1
            </code>
        </div>
    {:else}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-zinc-800 text-left text-zinc-500">
                        <th class="px-4 py-3">Zone ID</th>
                        <th class="px-4 py-3">Name</th>
                        <th class="px-4 py-3">Caller Reference</th>
                        <th class="px-4 py-3">Record Sets</th>
                    </tr>
                </thead>
                <tbody>
                    {#each zones as zone}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                            <td class="px-4 py-3 font-mono text-orange-400 text-xs">{zone.id}</td>
                            <td class="px-4 py-3 font-mono text-zinc-100">{zone.name}</td>
                            <td class="px-4 py-3 text-zinc-400">{zone.callerReference}</td>
                            <td class="px-4 py-3 text-zinc-400">{zone.recordSetCount}</td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    {/if}
</div>
