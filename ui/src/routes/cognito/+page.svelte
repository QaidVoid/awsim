<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listUserPools, createUserPool, deleteUserPool,
        listCognitoUsers, adminCreateUser, adminDeleteUser,
        type CognitoUserPool, type CognitoUser,
    } from '$lib/aws';

    let pools = $state<CognitoUserPool[]>([]);
    let poolsLoading = $state(true);
    let poolsError = $state<string | null>(null);

    let selectedPool = $state<CognitoUserPool | null>(null);

    let users = $state<CognitoUser[]>([]);
    let usersLoading = $state(false);
    let usersError = $state<string | null>(null);

    let showCreatePool = $state(false);
    let newPoolName = $state('');
    let creatingPool = $state(false);
    let createPoolError = $state<string | null>(null);
    let confirmDeletePool = $state<string | null>(null);

    let showCreateUser = $state(false);
    let newUsername = $state('');
    let creatingUser = $state(false);
    let createUserError = $state<string | null>(null);
    let confirmDeleteUser = $state<string | null>(null);

    async function loadPools() {
        poolsLoading = true;
        poolsError = null;
        try {
            const data = await listUserPools();
            pools = data.userPools;
        } catch {
            poolsError = 'Could not connect to AWSim. Is it running on port 4566?';
        } finally {
            poolsLoading = false;
        }
    }

    async function handleCreatePool() {
        if (!newPoolName.trim()) return;
        creatingPool = true;
        createPoolError = null;
        try {
            await createUserPool(newPoolName.trim());
            newPoolName = '';
            showCreatePool = false;
            await loadPools();
        } catch (e) {
            createPoolError = e instanceof Error ? e.message : 'Failed to create user pool';
        } finally {
            creatingPool = false;
        }
    }

    async function handleDeletePool(poolId: string) {
        try {
            await deleteUserPool(poolId);
            confirmDeletePool = null;
            if (selectedPool?.id === poolId) {
                selectedPool = null;
                users = [];
            }
            await loadPools();
        } catch (e) {
            poolsError = e instanceof Error ? e.message : 'Failed to delete user pool';
        }
    }

    async function selectPool(pool: CognitoUserPool) {
        selectedPool = pool;
        users = [];
        usersError = null;
        showCreateUser = false;
        await loadPoolUsers(pool.id);
    }

    async function loadPoolUsers(poolId: string) {
        usersLoading = true;
        usersError = null;
        try {
            const data = await listCognitoUsers(poolId);
            users = data.users;
        } catch (e) {
            usersError = e instanceof Error ? e.message : 'Failed to load users';
        } finally {
            usersLoading = false;
        }
    }

    async function handleCreateUser() {
        if (!selectedPool || !newUsername.trim()) return;
        creatingUser = true;
        createUserError = null;
        try {
            await adminCreateUser(selectedPool.id, newUsername.trim());
            newUsername = '';
            showCreateUser = false;
            await loadPoolUsers(selectedPool.id);
        } catch (e) {
            createUserError = e instanceof Error ? e.message : 'Failed to create user';
        } finally {
            creatingUser = false;
        }
    }

    async function handleDeleteUser(username: string) {
        if (!selectedPool) return;
        try {
            await adminDeleteUser(selectedPool.id, username);
            confirmDeleteUser = null;
            await loadPoolUsers(selectedPool.id);
        } catch (e) {
            usersError = e instanceof Error ? e.message : 'Failed to delete user';
        }
    }

    function formatDate(iso: string): string {
        if (!iso) return '—';
        try { return new Date(iso).toLocaleString(); } catch { return iso; }
    }

    onMount(loadPools);
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">Cognito — User Pools</h1>
            <p class="text-zinc-500 mt-1">User authentication and authorization. Manage user pools and identity.</p>
        </div>
        <button
            onclick={() => { showCreatePool = !showCreatePool; createPoolError = null; }}
            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
        >
            Create User Pool
        </button>
    </div>

    {#if showCreatePool}
        <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
            <h3 class="font-semibold mb-3">Create User Pool</h3>
            {#if createPoolError}
                <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createPoolError}</div>
            {/if}
            <input
                type="text"
                bind:value={newPoolName}
                onkeydown={(e) => e.key === 'Enter' && handleCreatePool()}
                class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                placeholder="my-user-pool"
            />
            <div class="flex gap-2 mt-3">
                <button
                    onclick={handleCreatePool}
                    disabled={creatingPool || !newPoolName.trim()}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                >
                    {creatingPool ? 'Creating...' : 'Create'}
                </button>
                <button
                    onclick={() => { showCreatePool = false; createPoolError = null; newPoolName = ''; }}
                    class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                >
                    Cancel
                </button>
            </div>
        </div>
    {/if}

    {#if poolsLoading}
        <div class="text-zinc-500">Loading...</div>
    {:else if poolsError}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{poolsError}</div>
    {:else if pools.length === 0 && !showCreatePool}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No user pools yet.</p>
            <button onclick={() => showCreatePool = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                Create your first pool
            </button>
        </div>
    {:else}
        <div class="flex gap-4">
            <!-- Pool list -->
            <div class="w-72 shrink-0">
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                    {#each pools as pool}
                        <div class="border-b border-zinc-800/50 last:border-0 {selectedPool?.id === pool.id ? 'bg-zinc-800' : 'hover:bg-zinc-800/40'} transition-colors">
                            <div class="px-4 py-3 flex items-start justify-between gap-2">
                                <button class="flex-1 text-left min-w-0" onclick={() => selectPool(pool)}>
                                    <div class="font-mono text-orange-400 text-sm truncate">{pool.name}</div>
                                    <div class="text-xs text-zinc-500 mt-0.5 font-mono truncate">{pool.id}</div>
                                    <div class="text-xs text-zinc-600 mt-0.5">{pool.status}</div>
                                </button>
                                <button
                                    onclick={(e) => { e.stopPropagation(); confirmDeletePool = pool.id; }}
                                    class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs shrink-0 transition-colors"
                                >
                                    Delete
                                </button>
                            </div>
                            {#if confirmDeletePool === pool.id}
                                <div class="px-4 pb-3 bg-red-900/10 border-t border-red-900/30">
                                    <p class="text-xs text-red-400 mb-2">Delete "{pool.name}"?</p>
                                    <div class="flex gap-2">
                                        <button onclick={() => handleDeletePool(pool.id)} class="px-2 py-1 bg-red-700 hover:bg-red-600 rounded text-xs font-medium">Confirm</button>
                                        <button onclick={() => confirmDeletePool = null} class="px-2 py-1 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                    </div>
                                </div>
                            {/if}
                        </div>
                    {/each}
                </div>
            </div>

            <!-- Pool detail panel -->
            <div class="flex-1 min-w-0">
                {#if selectedPool}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                        <!-- Pool header -->
                        <div class="px-4 py-3 border-b border-zinc-800 flex items-center justify-between">
                            <div>
                                <h2 class="font-semibold text-orange-400 font-mono">{selectedPool.name}</h2>
                                <div class="text-xs text-zinc-500 mt-0.5 font-mono">{selectedPool.id}</div>
                            </div>
                            <button
                                onclick={() => { showCreateUser = !showCreateUser; createUserError = null; newUsername = ''; }}
                                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
                            >
                                Create User
                            </button>
                        </div>

                        {#if showCreateUser}
                            <div class="px-4 py-3 border-b border-zinc-800 bg-zinc-800/50">
                                <h4 class="text-sm font-medium mb-2">Create User</h4>
                                {#if createUserError}
                                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs mb-2">{createUserError}</div>
                                {/if}
                                <div class="flex gap-2">
                                    <input
                                        type="text"
                                        bind:value={newUsername}
                                        onkeydown={(e) => e.key === 'Enter' && handleCreateUser()}
                                        class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500"
                                        placeholder="username"
                                    />
                                    <button
                                        onclick={handleCreateUser}
                                        disabled={creatingUser || !newUsername.trim()}
                                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm font-medium transition-colors"
                                    >
                                        {creatingUser ? 'Creating...' : 'Create'}
                                    </button>
                                    <button
                                        onclick={() => { showCreateUser = false; createUserError = null; newUsername = ''; }}
                                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                                    >
                                        Cancel
                                    </button>
                                </div>
                            </div>
                        {/if}

                        <!-- Users table -->
                        {#if usersLoading}
                            <div class="p-4 text-zinc-500 text-sm">Loading users...</div>
                        {:else if usersError}
                            <div class="p-4 text-red-400 text-sm">{usersError}</div>
                        {:else if users.length === 0}
                            <div class="p-8 text-center text-zinc-500 text-sm">
                                No users in this pool. Click "Create User" to add one.
                            </div>
                        {:else}
                            <table class="w-full text-sm">
                                <thead>
                                    <tr class="border-b border-zinc-800 text-left text-zinc-500">
                                        <th class="px-4 py-3">Username</th>
                                        <th class="px-4 py-3">Status</th>
                                        <th class="px-4 py-3">Created</th>
                                        <th class="px-4 py-3"></th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {#each users as user}
                                        <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                            <td class="px-4 py-3 font-mono text-orange-400">{user.username}</td>
                                            <td class="px-4 py-3">
                                                <span class="px-2 py-0.5 rounded text-xs font-medium {user.status === 'CONFIRMED' ? 'bg-green-900/30 text-green-400' : user.status === 'FORCE_CHANGE_PASSWORD' ? 'bg-yellow-900/30 text-yellow-400' : 'bg-zinc-700 text-zinc-400'}">
                                                    {user.status || 'UNKNOWN'}
                                                </span>
                                            </td>
                                            <td class="px-4 py-3 text-zinc-400 text-xs">{formatDate(user.createDate)}</td>
                                            <td class="px-4 py-3">
                                                {#if confirmDeleteUser === user.username}
                                                    <div class="flex items-center gap-1">
                                                        <button onclick={() => handleDeleteUser(user.username)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                                        <button onclick={() => confirmDeleteUser = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                                    </div>
                                                {:else}
                                                    <button onclick={() => confirmDeleteUser = user.username} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                                {/if}
                                            </td>
                                        </tr>
                                    {/each}
                                </tbody>
                            </table>
                        {/if}
                    </div>
                {:else}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-500 text-sm">
                        Select a user pool to view details and manage users.
                    </div>
                {/if}
            </div>
        </div>
    {/if}
</div>
