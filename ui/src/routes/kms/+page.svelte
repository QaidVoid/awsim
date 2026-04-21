<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listKeys, describeKey, createKey, listAliases, createAlias, kmsEncrypt, kmsDecrypt,
        type KmsKey, type KmsKeyDetail, type KmsAlias,
    } from '$lib/aws';

    let activeTab = $state<'keys' | 'aliases' | 'crypto'>('keys');

    // --- Keys ---
    let keys = $state<KmsKey[]>([]);
    let keyDetails = $state<Record<string, KmsKeyDetail>>({});
    let keysLoading = $state(false);
    let keysError = $state<string | null>(null);
    let showCreateKey = $state(false);
    let newKeyDescription = $state('');
    let creatingKey = $state(false);
    let createKeyError = $state<string | null>(null);

    // --- Aliases ---
    let aliases = $state<KmsAlias[]>([]);
    let aliasesLoading = $state(false);
    let aliasesError = $state<string | null>(null);
    let showCreateAlias = $state(false);
    let newAliasName = $state('alias/');
    let newAliasTargetKeyId = $state('');
    let creatingAlias = $state(false);
    let createAliasError = $state<string | null>(null);

    // --- Encrypt/Decrypt ---
    let cryptoKeyId = $state('');
    let plaintext = $state('');
    let ciphertext = $state('');
    let decryptInput = $state('');
    let decryptOutput = $state('');
    let encrypting = $state(false);
    let decrypting = $state(false);
    let encryptError = $state<string | null>(null);
    let decryptError = $state<string | null>(null);

    async function loadKeys() {
        keysLoading = true;
        keysError = null;
        try {
            const data = await listKeys();
            keys = data.keys;
            // Load details for each key in background
            for (const key of data.keys) {
                describeKey(key.keyId).then((detail) => {
                    keyDetails = { ...keyDetails, [key.keyId]: detail };
                }).catch(() => {});
            }
        } catch (e) {
            keysError = e instanceof Error ? e.message : 'Failed to load keys';
        } finally {
            keysLoading = false;
        }
    }

    async function handleCreateKey() {
        creatingKey = true;
        createKeyError = null;
        try {
            await createKey(newKeyDescription.trim() || undefined);
            newKeyDescription = '';
            showCreateKey = false;
            await loadKeys();
        } catch (e) {
            createKeyError = e instanceof Error ? e.message : 'Failed to create key';
        } finally {
            creatingKey = false;
        }
    }

    async function loadAliases() {
        aliasesLoading = true;
        aliasesError = null;
        try {
            const data = await listAliases();
            aliases = data.aliases;
        } catch (e) {
            aliasesError = e instanceof Error ? e.message : 'Failed to load aliases';
        } finally {
            aliasesLoading = false;
        }
    }

    async function handleCreateAlias() {
        if (!newAliasName.trim() || !newAliasTargetKeyId.trim()) return;
        creatingAlias = true;
        createAliasError = null;
        try {
            await createAlias(newAliasName.trim(), newAliasTargetKeyId.trim());
            newAliasName = 'alias/';
            newAliasTargetKeyId = '';
            showCreateAlias = false;
            await loadAliases();
        } catch (e) {
            createAliasError = e instanceof Error ? e.message : 'Failed to create alias';
        } finally {
            creatingAlias = false;
        }
    }

    async function handleEncrypt() {
        if (!cryptoKeyId.trim() || !plaintext.trim()) return;
        encrypting = true;
        encryptError = null;
        ciphertext = '';
        try {
            const result = await kmsEncrypt(cryptoKeyId.trim(), plaintext);
            ciphertext = result.ciphertextBlob;
        } catch (e) {
            encryptError = e instanceof Error ? e.message : 'Encryption failed';
        } finally {
            encrypting = false;
        }
    }

    async function handleDecrypt() {
        if (!decryptInput.trim()) return;
        decrypting = true;
        decryptError = null;
        decryptOutput = '';
        try {
            const result = await kmsDecrypt(decryptInput.trim());
            decryptOutput = result.plaintext;
        } catch (e) {
            decryptError = e instanceof Error ? e.message : 'Decryption failed';
        } finally {
            decrypting = false;
        }
    }

    function switchTab(tab: 'keys' | 'aliases' | 'crypto') {
        activeTab = tab;
        if (tab === 'keys' && keys.length === 0 && !keysLoading) loadKeys();
        if (tab === 'aliases' && aliases.length === 0 && !aliasesLoading) loadAliases();
        if (tab === 'crypto' && keys.length === 0 && !keysLoading) loadKeys();
    }

    function formatDate(iso: string): string {
        try { return new Date(iso).toLocaleString(); } catch { return iso; }
    }

    function shortId(id: string): string {
        return id.length > 12 ? `${id.slice(0, 8)}...${id.slice(-4)}` : id;
    }

    onMount(() => loadKeys());
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">KMS — Key Management Service</h1>
            <p class="text-zinc-500 mt-1">Create and manage cryptographic keys for data encryption.</p>
        </div>
        {#if activeTab === 'keys'}
            <button
                onclick={() => { showCreateKey = !showCreateKey; createKeyError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Key
            </button>
        {:else if activeTab === 'aliases'}
            <button
                onclick={() => { showCreateAlias = !showCreateAlias; createAliasError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Alias
            </button>
        {/if}
    </div>

    <!-- Tab nav -->
    <div class="flex gap-1 mb-4 border-b border-zinc-800">
        {#each [['keys', 'Keys'], ['aliases', 'Aliases'], ['crypto', 'Encrypt / Decrypt']] as [tab, label]}
            <button
                onclick={() => switchTab(tab as 'keys' | 'aliases' | 'crypto')}
                class="px-4 py-2 text-sm font-medium border-b-2 transition-colors {activeTab === tab ? 'border-orange-400 text-orange-400' : 'border-transparent text-zinc-500 hover:text-zinc-300'}"
            >
                {label}
            </button>
        {/each}
    </div>

    <!-- Keys tab -->
    {#if activeTab === 'keys'}
        {#if showCreateKey}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create Key</h3>
                {#if createKeyError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createKeyError}</div>
                {/if}
                <div class="mb-3">
                    <label class="block text-xs text-zinc-400 mb-1">Description (optional)</label>
                    <input
                        type="text"
                        bind:value={newKeyDescription}
                        onkeydown={(e) => e.key === 'Enter' && handleCreateKey()}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        placeholder="My encryption key"
                    />
                </div>
                <div class="flex gap-2">
                    <button
                        onclick={handleCreateKey}
                        disabled={creatingKey}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                    >
                        {creatingKey ? 'Creating...' : 'Create'}
                    </button>
                    <button
                        onclick={() => { showCreateKey = false; createKeyError = null; newKeyDescription = ''; }}
                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        {/if}

        {#if keysLoading}
            <div class="text-zinc-500">Loading...</div>
        {:else if keysError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{keysError}</div>
        {:else if keys.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No KMS keys yet.</p>
                <button onclick={() => showCreateKey = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                    Create your first key
                </button>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Key ID</th>
                            <th class="px-4 py-3">Description</th>
                            <th class="px-4 py-3">State</th>
                            <th class="px-4 py-3">Created</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each keys as key}
                            {@const detail = keyDetails[key.keyId]}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400 text-xs" title={key.keyId}>{shortId(key.keyId)}</td>
                                <td class="px-4 py-3 text-zinc-300 text-xs">
                                    {#if detail?.description}
                                        {detail.description}
                                    {:else}
                                        <span class="text-zinc-600">—</span>
                                    {/if}
                                </td>
                                <td class="px-4 py-3">
                                    {#if detail}
                                        <span class="px-1.5 py-0.5 rounded text-xs font-medium {detail.keyState === 'Enabled' ? 'bg-green-900/30 text-green-400' : 'bg-zinc-800 text-zinc-500'}">
                                            {detail.keyState}
                                        </span>
                                    {:else}
                                        <span class="text-zinc-600 text-xs">—</span>
                                    {/if}
                                </td>
                                <td class="px-4 py-3 text-zinc-400 text-xs">{detail ? formatDate(detail.creationDate) : '—'}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}

    <!-- Aliases tab -->
    {#if activeTab === 'aliases'}
        {#if showCreateAlias}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create Alias</h3>
                {#if createAliasError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createAliasError}</div>
                {/if}
                <div class="mb-3">
                    <label class="block text-xs text-zinc-400 mb-1">Alias Name (must start with alias/)</label>
                    <input
                        type="text"
                        bind:value={newAliasName}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500"
                        placeholder="alias/my-key"
                    />
                </div>
                <div class="mb-3">
                    <label class="block text-xs text-zinc-400 mb-1">Target Key ID</label>
                    <input
                        type="text"
                        bind:value={newAliasTargetKeyId}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500"
                        placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
                    />
                    {#if keys.length > 0}
                        <div class="mt-2">
                            <span class="text-xs text-zinc-500">Quick select: </span>
                            {#each keys as key}
                                <button
                                    onclick={() => newAliasTargetKeyId = key.keyId}
                                    class="text-xs text-orange-400 hover:text-orange-300 font-mono mr-2"
                                    title={key.keyId}
                                >{shortId(key.keyId)}</button>
                            {/each}
                        </div>
                    {/if}
                </div>
                <div class="flex gap-2">
                    <button
                        onclick={handleCreateAlias}
                        disabled={creatingAlias || !newAliasName.trim() || !newAliasTargetKeyId.trim()}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                    >
                        {creatingAlias ? 'Creating...' : 'Create'}
                    </button>
                    <button
                        onclick={() => { showCreateAlias = false; createAliasError = null; }}
                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        {/if}

        {#if aliasesLoading}
            <div class="text-zinc-500">Loading...</div>
        {:else if aliasesError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{aliasesError}</div>
        {:else if aliases.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No aliases yet.</p>
                <button onclick={() => showCreateAlias = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                    Create your first alias
                </button>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Alias Name</th>
                            <th class="px-4 py-3">Target Key ID</th>
                            <th class="px-4 py-3">Alias ARN</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each aliases as alias}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400 text-sm">{alias.aliasName}</td>
                                <td class="px-4 py-3 font-mono text-zinc-400 text-xs">
                                    {#if alias.targetKeyId}
                                        {alias.targetKeyId}
                                    {:else}
                                        <span class="text-zinc-600">—</span>
                                    {/if}
                                </td>
                                <td class="px-4 py-3 font-mono text-zinc-500 text-xs truncate max-w-xs">{alias.aliasArn}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}

    <!-- Encrypt/Decrypt tab -->
    {#if activeTab === 'crypto'}
        <div class="grid grid-cols-2 gap-4">
            <!-- Encrypt panel -->
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4">
                <h3 class="font-semibold mb-3">Encrypt</h3>
                <div class="mb-3">
                    <label class="block text-xs text-zinc-400 mb-1">Key</label>
                    {#if keys.length > 0}
                        <select
                            bind:value={cryptoKeyId}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500"
                        >
                            <option value="">Select a key...</option>
                            {#each keys as key}
                                {@const detail = keyDetails[key.keyId]}
                                <option value={key.keyId}>
                                    {shortId(key.keyId)}{detail?.description ? ` — ${detail.description}` : ''}
                                </option>
                            {/each}
                        </select>
                    {:else}
                        <input
                            type="text"
                            bind:value={cryptoKeyId}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500"
                            placeholder="Key ID or ARN"
                        />
                    {/if}
                </div>
                <div class="mb-3">
                    <label class="block text-xs text-zinc-400 mb-1">Plaintext</label>
                    <textarea
                        bind:value={plaintext}
                        rows="4"
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y"
                        placeholder="Text to encrypt..."
                    ></textarea>
                </div>
                {#if encryptError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{encryptError}</div>
                {/if}
                <button
                    onclick={handleEncrypt}
                    disabled={encrypting || !cryptoKeyId.trim() || !plaintext.trim()}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors mb-3"
                >
                    {encrypting ? 'Encrypting...' : 'Encrypt'}
                </button>
                {#if ciphertext}
                    <div>
                        <label class="block text-xs text-zinc-400 mb-1">Ciphertext (base64)</label>
                        <textarea
                            value={ciphertext}
                            readonly
                            rows="4"
                            class="w-full bg-zinc-800 border border-zinc-600 rounded px-3 py-2 text-xs font-mono text-green-400 resize-y"
                        ></textarea>
                    </div>
                {/if}
            </div>

            <!-- Decrypt panel -->
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4">
                <h3 class="font-semibold mb-3">Decrypt</h3>
                <div class="mb-3">
                    <label class="block text-xs text-zinc-400 mb-1">Ciphertext (base64)</label>
                    <textarea
                        bind:value={decryptInput}
                        rows="4"
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y"
                        placeholder="Paste ciphertext blob..."
                    ></textarea>
                </div>
                {#if decryptError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{decryptError}</div>
                {/if}
                <button
                    onclick={handleDecrypt}
                    disabled={decrypting || !decryptInput.trim()}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors mb-3"
                >
                    {decrypting ? 'Decrypting...' : 'Decrypt'}
                </button>
                {#if decryptOutput}
                    <div>
                        <label class="block text-xs text-zinc-400 mb-1">Decrypted Plaintext</label>
                        <textarea
                            value={decryptOutput}
                            readonly
                            rows="4"
                            class="w-full bg-zinc-800 border border-zinc-600 rounded px-3 py-2 text-sm font-mono text-green-400 resize-y"
                        ></textarea>
                    </div>
                {/if}
            </div>
        </div>
    {/if}
</div>
