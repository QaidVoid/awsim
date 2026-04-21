<script lang="ts">
    import { onMount, onDestroy } from 'svelte';
    import { getRequestLog, clearRequestLog, type RequestLogEntry } from '$lib/aws';

    let entries = $state<RequestLogEntry[]>([]);
    let intervalId: ReturnType<typeof setInterval>;

    function refresh() {
        entries = getRequestLog();
    }

    function handleClear() {
        clearRequestLog();
        entries = [];
    }

    function statusClass(status: number): string {
        if (status >= 200 && status < 300) return 'text-green-400';
        if (status >= 400 && status < 500) return 'text-yellow-400';
        if (status >= 500) return 'text-red-400';
        if (status === 0) return 'text-zinc-600';
        return 'text-zinc-400';
    }

    function formatTs(iso: string): string {
        try {
            const d = new Date(iso);
            return d.toLocaleTimeString(undefined, { hour12: false }) + '.' + String(d.getMilliseconds()).padStart(3, '0');
        } catch {
            return iso;
        }
    }

    function formatDuration(ms: number): string {
        if (ms >= 1000) return `${(ms / 1000).toFixed(2)}s`;
        return `${ms}ms`;
    }

    onMount(() => {
        refresh();
        intervalId = setInterval(refresh, 1000);
    });

    onDestroy(() => clearInterval(intervalId));
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">API Request Log</h1>
            <p class="text-zinc-500 mt-1">Client-side log of all AWS API calls made this session.</p>
        </div>
        <div class="flex items-center gap-3">
            <span class="text-sm text-zinc-500">{entries.length} request{entries.length !== 1 ? 's' : ''}</span>
            <button
                onclick={refresh}
                class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
            >
                Refresh
            </button>
            <button
                onclick={handleClear}
                disabled={entries.length === 0}
                class="px-3 py-1.5 bg-red-900/50 hover:bg-red-900 border border-red-800 text-red-400 hover:text-red-300 disabled:opacity-40 disabled:cursor-not-allowed rounded text-sm transition-colors"
            >
                Clear
            </button>
        </div>
    </div>

    {#if entries.length === 0}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-12 text-center">
            <p class="text-zinc-500 text-lg mb-2">No requests logged yet.</p>
            <p class="text-zinc-600 text-sm">Navigate to any service page to start making API calls.</p>
        </div>
    {:else}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b border-zinc-800 text-left text-zinc-500 text-xs uppercase tracking-wide">
                        <th class="px-4 py-3 w-8">#</th>
                        <th class="px-4 py-3">Time</th>
                        <th class="px-4 py-3">Service</th>
                        <th class="px-4 py-3">Operation</th>
                        <th class="px-4 py-3">Method</th>
                        <th class="px-4 py-3">Status</th>
                        <th class="px-4 py-3">Duration</th>
                    </tr>
                </thead>
                <tbody>
                    {#each entries as entry}
                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30 font-mono text-xs">
                            <td class="px-4 py-2.5 text-zinc-600">{entry.id}</td>
                            <td class="px-4 py-2.5 text-zinc-500">{formatTs(entry.timestamp)}</td>
                            <td class="px-4 py-2.5 text-orange-400">{entry.service}</td>
                            <td class="px-4 py-2.5 text-zinc-200">{entry.operation}</td>
                            <td class="px-4 py-2.5 text-zinc-400">{entry.method}</td>
                            <td class="px-4 py-2.5 {statusClass(entry.status)}">
                                {entry.status === 0 ? 'ERR' : entry.status}
                            </td>
                            <td class="px-4 py-2.5 text-zinc-400">{formatDuration(entry.duration)}</td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    {/if}

    <p class="mt-4 text-xs text-zinc-600">
        Auto-refreshes every second. Stores up to 500 entries per session. Log resets on page reload.
    </p>
</div>
