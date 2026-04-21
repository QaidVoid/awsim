<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listUsers, createUser, deleteUser,
        listRoles, createRole, deleteRole,
        listPolicies,
        type IamUser, type IamRole, type IamPolicy,
    } from '$lib/aws';

    let activeTab = $state<'users' | 'roles' | 'policies'>('users');

    // ---- Users ----
    let users = $state<IamUser[]>([]);
    let usersLoading = $state(false);
    let usersError = $state<string | null>(null);
    let showCreateUser = $state(false);
    let newUserName = $state('');
    let creatingUser = $state(false);
    let createUserError = $state<string | null>(null);
    let confirmDeleteUser = $state<string | null>(null);

    // ---- Roles ----
    let roles = $state<IamRole[]>([]);
    let rolesLoading = $state(false);
    let rolesError = $state<string | null>(null);
    let showCreateRole = $state(false);
    let newRoleName = $state('');
    let newRolePolicy = $state(JSON.stringify({
        Version: '2012-10-17',
        Statement: [{ Effect: 'Allow', Principal: { Service: 'lambda.amazonaws.com' }, Action: 'sts:AssumeRole' }]
    }, null, 2));
    let creatingRole = $state(false);
    let createRoleError = $state<string | null>(null);
    let confirmDeleteRole = $state<string | null>(null);

    // ---- Policies ----
    let policies = $state<IamPolicy[]>([]);
    let policiesLoading = $state(false);
    let policiesError = $state<string | null>(null);

    async function loadUsers() {
        usersLoading = true;
        usersError = null;
        try {
            const data = await listUsers();
            users = data.users;
        } catch (e) {
            usersError = e instanceof Error ? e.message : 'Failed to load users';
        } finally {
            usersLoading = false;
        }
    }

    async function handleCreateUser() {
        if (!newUserName.trim()) return;
        creatingUser = true;
        createUserError = null;
        try {
            await createUser(newUserName.trim());
            newUserName = '';
            showCreateUser = false;
            await loadUsers();
        } catch (e) {
            createUserError = e instanceof Error ? e.message : 'Failed to create user';
        } finally {
            creatingUser = false;
        }
    }

    async function handleDeleteUser(userName: string) {
        try {
            await deleteUser(userName);
            confirmDeleteUser = null;
            await loadUsers();
        } catch (e) {
            usersError = e instanceof Error ? e.message : 'Failed to delete user';
        }
    }

    async function loadRoles() {
        rolesLoading = true;
        rolesError = null;
        try {
            const data = await listRoles();
            roles = data.roles;
        } catch (e) {
            rolesError = e instanceof Error ? e.message : 'Failed to load roles';
        } finally {
            rolesLoading = false;
        }
    }

    async function handleCreateRole() {
        if (!newRoleName.trim()) return;
        creatingRole = true;
        createRoleError = null;
        try {
            await createRole(newRoleName.trim(), newRolePolicy);
            newRoleName = '';
            showCreateRole = false;
            await loadRoles();
        } catch (e) {
            createRoleError = e instanceof Error ? e.message : 'Failed to create role';
        } finally {
            creatingRole = false;
        }
    }

    async function handleDeleteRole(roleName: string) {
        try {
            await deleteRole(roleName);
            confirmDeleteRole = null;
            await loadRoles();
        } catch (e) {
            rolesError = e instanceof Error ? e.message : 'Failed to delete role';
        }
    }

    async function loadPolicies() {
        policiesLoading = true;
        policiesError = null;
        try {
            const data = await listPolicies();
            policies = data.policies;
        } catch (e) {
            policiesError = e instanceof Error ? e.message : 'Failed to load policies';
        } finally {
            policiesLoading = false;
        }
    }

    function switchTab(tab: 'users' | 'roles' | 'policies') {
        activeTab = tab;
        if (tab === 'users' && users.length === 0 && !usersLoading) loadUsers();
        if (tab === 'roles' && roles.length === 0 && !rolesLoading) loadRoles();
        if (tab === 'policies' && policies.length === 0 && !policiesLoading) loadPolicies();
    }

    function formatDate(iso: string): string {
        try { return new Date(iso).toLocaleString(); } catch { return iso; }
    }

    onMount(() => loadUsers());
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">IAM — Identity &amp; Access Management</h1>
            <p class="text-zinc-500 mt-1">Manage users, roles, and policies for access control.</p>
        </div>
        {#if activeTab === 'users'}
            <button
                onclick={() => { showCreateUser = !showCreateUser; createUserError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create User
            </button>
        {:else if activeTab === 'roles'}
            <button
                onclick={() => { showCreateRole = !showCreateRole; createRoleError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Role
            </button>
        {/if}
    </div>

    <!-- Tab nav -->
    <div class="flex gap-1 mb-4 border-b border-zinc-800">
        {#each ['users', 'roles', 'policies'] as tab}
            <button
                onclick={() => switchTab(tab as 'users' | 'roles' | 'policies')}
                class="px-4 py-2 text-sm font-medium border-b-2 transition-colors {activeTab === tab ? 'border-orange-400 text-orange-400' : 'border-transparent text-zinc-500 hover:text-zinc-300'}"
            >
                {tab.charAt(0).toUpperCase() + tab.slice(1)}
            </button>
        {/each}
    </div>

    <!-- Users tab -->
    {#if activeTab === 'users'}
        {#if showCreateUser}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create User</h3>
                {#if createUserError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createUserError}</div>
                {/if}
                <input
                    type="text"
                    bind:value={newUserName}
                    onkeydown={(e) => e.key === 'Enter' && handleCreateUser()}
                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                    placeholder="username"
                />
                <div class="flex gap-2 mt-3">
                    <button
                        onclick={handleCreateUser}
                        disabled={creatingUser || !newUserName.trim()}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                    >
                        {creatingUser ? 'Creating...' : 'Create'}
                    </button>
                    <button
                        onclick={() => { showCreateUser = false; createUserError = null; newUserName = ''; }}
                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        {/if}

        {#if usersLoading}
            <div class="text-zinc-500">Loading users...</div>
        {:else if usersError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{usersError}</div>
        {:else if users.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No IAM users yet.</p>
                <button onclick={() => showCreateUser = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                    Create your first user
                </button>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Username</th>
                            <th class="px-4 py-3">User ID</th>
                            <th class="px-4 py-3">ARN</th>
                            <th class="px-4 py-3">Created</th>
                            <th class="px-4 py-3"></th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each users as user}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400">{user.userName}</td>
                                <td class="px-4 py-3 text-zinc-400 text-xs font-mono">{user.userId}</td>
                                <td class="px-4 py-3 text-zinc-400 text-xs font-mono truncate max-w-xs">{user.arn}</td>
                                <td class="px-4 py-3 text-zinc-400 text-xs">{formatDate(user.createDate)}</td>
                                <td class="px-4 py-3">
                                    {#if confirmDeleteUser === user.userName}
                                        <div class="flex items-center gap-1">
                                            <button onclick={() => handleDeleteUser(user.userName)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteUser = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button onclick={() => confirmDeleteUser = user.userName} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                    {/if}
                                </td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}

    <!-- Roles tab -->
    {#if activeTab === 'roles'}
        {#if showCreateRole}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create Role</h3>
                {#if createRoleError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createRoleError}</div>
                {/if}
                <div class="mb-3">
                    <label for="role-name" class="block text-xs text-zinc-400 mb-1">Role Name</label>
                    <input
                        id="role-name"
                        type="text"
                        bind:value={newRoleName}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        placeholder="my-role"
                    />
                </div>
                <div class="mb-3">
                    <label for="role-policy" class="block text-xs text-zinc-400 mb-1">Assume Role Policy Document (JSON)</label>
                    <textarea
                        id="role-policy"
                        bind:value={newRolePolicy}
                        rows="8"
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y"
                    ></textarea>
                </div>
                <div class="flex gap-2">
                    <button
                        onclick={handleCreateRole}
                        disabled={creatingRole || !newRoleName.trim()}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                    >
                        {creatingRole ? 'Creating...' : 'Create'}
                    </button>
                    <button
                        onclick={() => { showCreateRole = false; createRoleError = null; newRoleName = ''; }}
                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        {/if}

        {#if rolesLoading}
            <div class="text-zinc-500">Loading roles...</div>
        {:else if rolesError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{rolesError}</div>
        {:else if roles.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No IAM roles yet.</p>
                <button onclick={() => showCreateRole = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                    Create your first role
                </button>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Role Name</th>
                            <th class="px-4 py-3">Role ID</th>
                            <th class="px-4 py-3">ARN</th>
                            <th class="px-4 py-3"></th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each roles as role}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400">{role.roleName}</td>
                                <td class="px-4 py-3 text-zinc-400 text-xs font-mono">{role.roleId}</td>
                                <td class="px-4 py-3 text-zinc-400 text-xs font-mono truncate max-w-xs">{role.arn}</td>
                                <td class="px-4 py-3">
                                    {#if confirmDeleteRole === role.roleName}
                                        <div class="flex items-center gap-1">
                                            <button onclick={() => handleDeleteRole(role.roleName)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteRole = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button onclick={() => confirmDeleteRole = role.roleName} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                    {/if}
                                </td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}

    <!-- Policies tab -->
    {#if activeTab === 'policies'}
        {#if policiesLoading}
            <div class="text-zinc-500">Loading policies...</div>
        {:else if policiesError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{policiesError}</div>
        {:else if policies.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No local IAM policies yet.</p>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Policy Name</th>
                            <th class="px-4 py-3">ARN</th>
                            <th class="px-4 py-3">Attachments</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each policies as policy}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400">{policy.policyName}</td>
                                <td class="px-4 py-3 text-zinc-400 text-xs font-mono truncate max-w-sm">{policy.arn}</td>
                                <td class="px-4 py-3 text-zinc-400">{policy.attachmentCount}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}
</div>
