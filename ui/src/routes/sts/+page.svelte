<script lang="ts">
    import { onMount } from 'svelte';
    import { getCallerIdentity, assumeRole, type StsIdentity, type StsCredentials } from '$lib/aws';

    let identity = $state<StsIdentity | null>(null);
    let identityLoading = $state(true);
    let identityError = $state<string | null>(null);

    // Assume role form
    let roleArn = $state('arn:aws:iam::000000000000:role/my-role');
    let roleSessionName = $state('my-session');
    let assuming = $state(false);
    let assumeError = $state<string | null>(null);
    let credentials = $state<StsCredentials | null>(null);

    onMount(async () => {
        try {
            identity = await getCallerIdentity();
        } catch (e) {
            identityError = e instanceof Error ? e.message : 'Failed to fetch caller identity';
        } finally {
            identityLoading = false;
        }
    });

    async function handleAssumeRole() {
        if (!roleArn.trim() || !roleSessionName.trim()) return;
        assuming = true;
        assumeError = null;
        credentials = null;
        try {
            credentials = await assumeRole(roleArn.trim(), roleSessionName.trim());
        } catch (e) {
            assumeError = e instanceof Error ? e.message : 'AssumeRole failed';
        } finally {
            assuming = false;
        }
    }
</script>

<div class="p-6">
    <div class="mb-6">
        <h1 class="text-2xl font-bold">STS — Security Token Service</h1>
        <p class="text-zinc-500 mt-1">Manage temporary credentials and caller identity.</p>
    </div>

    <!-- Caller Identity Card -->
    <section class="mb-8">
        <h2 class="text-lg font-semibold mb-3 text-zinc-300">Caller Identity</h2>
        {#if identityLoading}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-6 text-zinc-500">Loading...</div>
        {:else if identityError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{identityError}</div>
        {:else if identity}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-6">
                <dl class="grid grid-cols-1 gap-4">
                    <div class="flex flex-col gap-1">
                        <dt class="text-xs text-zinc-500 uppercase tracking-wide">Account ID</dt>
                        <dd class="font-mono text-orange-400 text-lg">{identity.account}</dd>
                    </div>
                    <div class="flex flex-col gap-1">
                        <dt class="text-xs text-zinc-500 uppercase tracking-wide">ARN</dt>
                        <dd class="font-mono text-zinc-200 text-sm break-all">{identity.arn}</dd>
                    </div>
                    <div class="flex flex-col gap-1">
                        <dt class="text-xs text-zinc-500 uppercase tracking-wide">User ID</dt>
                        <dd class="font-mono text-zinc-200 text-sm">{identity.userId}</dd>
                    </div>
                </dl>
            </div>
        {/if}
    </section>

    <!-- Assume Role -->
    <section>
        <h2 class="text-lg font-semibold mb-3 text-zinc-300">Assume Role</h2>
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-6">
            {#if assumeError}
                <div class="bg-red-900/20 border border-red-800 rounded p-3 text-red-400 text-sm mb-4">{assumeError}</div>
            {/if}
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4 mb-4">
                <div>
                    <label for="role-arn" class="block text-xs text-zinc-400 mb-1">Role ARN</label>
                    <input
                        id="role-arn"
                        type="text"
                        bind:value={roleArn}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500"
                        placeholder="arn:aws:iam::123456789012:role/MyRole"
                    />
                </div>
                <div>
                    <label for="session-name" class="block text-xs text-zinc-400 mb-1">Session Name</label>
                    <input
                        id="session-name"
                        type="text"
                        bind:value={roleSessionName}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        placeholder="my-session"
                    />
                </div>
            </div>
            <button
                onclick={handleAssumeRole}
                disabled={assuming || !roleArn.trim() || !roleSessionName.trim()}
                class="px-4 py-2 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
            >
                {assuming ? 'Assuming...' : 'Assume Role'}
            </button>

            {#if credentials}
                <div class="mt-6 border-t border-zinc-800 pt-6">
                    <h3 class="text-sm font-semibold text-zinc-300 mb-3">Temporary Credentials</h3>
                    <dl class="grid grid-cols-1 gap-3 text-sm">
                        <div>
                            <dt class="text-xs text-zinc-500 mb-0.5">Access Key ID</dt>
                            <dd class="font-mono text-green-400 bg-zinc-800 rounded px-2 py-1 text-xs break-all">{credentials.accessKeyId}</dd>
                        </div>
                        <div>
                            <dt class="text-xs text-zinc-500 mb-0.5">Secret Access Key</dt>
                            <dd class="font-mono text-green-400 bg-zinc-800 rounded px-2 py-1 text-xs break-all">{credentials.secretAccessKey}</dd>
                        </div>
                        <div>
                            <dt class="text-xs text-zinc-500 mb-0.5">Session Token</dt>
                            <dd class="font-mono text-green-400 bg-zinc-800 rounded px-2 py-1 text-xs break-all">{credentials.sessionToken}</dd>
                        </div>
                        <div>
                            <dt class="text-xs text-zinc-500 mb-0.5">Expiration</dt>
                            <dd class="font-mono text-zinc-300 text-xs">{credentials.expiration}</dd>
                        </div>
                    </dl>
                </div>
            {/if}
        </div>
    </section>
</div>
