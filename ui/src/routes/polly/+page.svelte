<script lang="ts">
    import { onMount } from 'svelte';

    const BASE = 'http://localhost:4566';

    interface Voice { Id: string; Name: string; Gender: string; LanguageCode: string; LanguageName: string; }
    interface Lexicon { Name: string; Attributes?: { Alphabet?: string; LanguageCode?: string; }; }

    let voices = $state<Voice[]>([]);
    let lexicons = $state<Lexicon[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let synthText = $state('Hello from AWSim Polly!');
    let synthVoice = $state('Joanna');
    let synthFormat = $state('mp3');
    let synthesizing = $state(false);
    let audioUrl = $state<string | null>(null);
    let synthError = $state<string | null>(null);

    function authHeader() {
        return 'AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/polly/aws4_request, SignedHeaders=host, Signature=fake';
    }

    async function apiFetchJson(method: string, path: string, body?: unknown) {
        const res = await fetch(`${BASE}${path}`, {
            method,
            headers: {
                'Content-Type': 'application/json',
                'Authorization': authHeader(),
            },
            body: body === undefined ? undefined : JSON.stringify(body),
        });
        if (!res.ok) throw new Error(await res.text() || `HTTP ${res.status}`);
        const text = await res.text();
        return text ? JSON.parse(text) : {};
    }

    async function loadAll() {
        loading = true;
        error = null;
        try {
            const [v, l] = await Promise.all([
                apiFetchJson('GET', '/v1/voices'),
                apiFetchJson('GET', '/v1/lexicons'),
            ]);
            voices = v.Voices ?? [];
            lexicons = l.Lexicons ?? [];
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed';
        } finally {
            loading = false;
        }
    }

    async function synthesize() {
        if (!synthText.trim()) return;
        synthesizing = true;
        synthError = null;
        if (audioUrl) {
            URL.revokeObjectURL(audioUrl);
            audioUrl = null;
        }
        try {
            const res = await fetch(`${BASE}/v1/speech`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'Authorization': authHeader(),
                },
                body: JSON.stringify({
                    Text: synthText,
                    VoiceId: synthVoice,
                    OutputFormat: synthFormat,
                }),
            });
            if (!res.ok) throw new Error(await res.text() || `HTTP ${res.status}`);
            const blob = await res.blob();
            audioUrl = URL.createObjectURL(blob);
        } catch (e) {
            synthError = e instanceof Error ? e.message : 'Failed';
        } finally {
            synthesizing = false;
        }
    }

    function genderColor(g: string): string {
        if (g === 'Female') return 'text-pink-400';
        if (g === 'Male') return 'text-blue-400';
        return 'text-zinc-400';
    }

    onMount(loadAll);
</script>

<div class="p-6 max-w-6xl mx-auto">
    <div class="mb-6">
        <h1 class="text-2xl font-bold">Polly</h1>
        <p class="text-zinc-500 mt-1">Text-to-speech synthesis.</p>
    </div>

    <div class="mb-6 p-4 bg-zinc-900 border border-zinc-700 rounded-lg">
        <h2 class="text-sm font-semibold mb-3">Synthesize Speech</h2>
        {#if synthError}<p class="text-red-400 text-xs mb-2">{synthError}</p>{/if}
        <div class="space-y-3">
            <textarea bind:value={synthText} rows="3" class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm focus:outline-none focus:border-orange-500"></textarea>
            <div class="flex gap-2">
                <select bind:value={synthVoice} class="bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-sm focus:outline-none focus:border-orange-500">
                    {#each voices as v}
                        <option value={v.Id}>{v.Name} ({v.LanguageCode})</option>
                    {/each}
                </select>
                <select bind:value={synthFormat} class="bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-sm focus:outline-none focus:border-orange-500">
                    <option value="mp3">mp3</option>
                    <option value="ogg_vorbis">ogg_vorbis</option>
                    <option value="pcm">pcm</option>
                </select>
                <button onclick={synthesize} disabled={synthesizing || !synthText.trim()} class="px-4 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm font-medium">
                    {synthesizing ? 'Synthesizing...' : 'Synthesize'}
                </button>
            </div>
            {#if audioUrl}
                <audio controls src={audioUrl} class="w-full"></audio>
            {/if}
        </div>
    </div>

    {#if loading}
        <p class="text-zinc-500 text-sm">Loading...</p>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else}
        <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
            <div>
                <h2 class="text-sm font-semibold text-zinc-300 mb-2">Voices ({voices.length})</h2>
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                    <table class="w-full text-sm">
                        <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">Name</th><th class="px-4 py-3 text-xs">Language</th><th class="px-4 py-3 text-xs">Gender</th></tr></thead>
                        <tbody>
                            {#each voices as v}
                                <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                    <td class="px-4 py-3 text-zinc-200">{v.Name}</td>
                                    <td class="px-4 py-3 text-zinc-400 text-xs">{v.LanguageName} ({v.LanguageCode})</td>
                                    <td class="px-4 py-3 text-xs {genderColor(v.Gender)}">{v.Gender}</td>
                                </tr>
                            {/each}
                        </tbody>
                    </table>
                </div>
            </div>
            <div>
                <h2 class="text-sm font-semibold text-zinc-300 mb-2">Lexicons ({lexicons.length})</h2>
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                    <table class="w-full text-sm">
                        <thead><tr class="border-b border-zinc-800 text-left text-zinc-500"><th class="px-4 py-3 text-xs">Name</th><th class="px-4 py-3 text-xs">Alphabet</th><th class="px-4 py-3 text-xs">Language</th></tr></thead>
                        <tbody>
                            {#each lexicons as l}
                                <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                    <td class="px-4 py-3 text-zinc-200">{l.Name}</td>
                                    <td class="px-4 py-3 text-zinc-400 text-xs">{l.Attributes?.Alphabet ?? ''}</td>
                                    <td class="px-4 py-3 text-zinc-400 text-xs">{l.Attributes?.LanguageCode ?? ''}</td>
                                </tr>
                            {/each}
                            {#if lexicons.length === 0}<tr><td colspan="3" class="px-4 py-8 text-center text-zinc-500">No lexicons.</td></tr>{/if}
                        </tbody>
                    </table>
                </div>
            </div>
        </div>
    {/if}
</div>
