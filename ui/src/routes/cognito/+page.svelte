<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listUserPools, createUserPool, deleteUserPool, describeUserPool,
        listUserPoolClients, createUserPoolClient, describeUserPoolClient, deleteUserPoolClient,
        listCognitoUsers, adminCreateUser, adminDeleteUser, adminGetUser,
        adminSetUserPassword, adminEnableUser, adminDisableUser, adminUpdateUserAttributes,
        listCognitoGroups, createCognitoGroup, deleteCognitoGroup,
        adminAddUserToGroup, adminRemoveUserFromGroup, listUsersInGroup, adminListGroupsForUser,
        cognitoSignUp, cognitoInitiateAuth, cognitoGetUser,
        listIdentityPools, createIdentityPool, deleteIdentityPool, describeIdentityPool,
        type CognitoUserPool, type CognitoUser, type CognitoUserPoolClient,
        type CognitoUserPoolClientDetail, type CognitoGroup, type CognitoUserDetail,
        type CognitoIdentityPool,
    } from '$lib/aws';

    // ---- Top-level tabs ----
    let topTab = $state<'userpools' | 'identitypools' | 'authtester'>('userpools');

    // ---- User Pools ----
    let pools = $state<CognitoUserPool[]>([]);
    let poolsLoading = $state(true);
    let poolsError = $state<string | null>(null);
    let selectedPool = $state<CognitoUserPool | null>(null);
    let showCreatePool = $state(false);
    let newPoolName = $state('');
    let newPoolMfa = $state('OFF');
    let creatingPool = $state(false);
    let createPoolError = $state<string | null>(null);
    let confirmDeletePool = $state<string | null>(null);

    // ---- Pool sub-tabs ----
    let poolSubTab = $state<'users' | 'groups' | 'clients' | 'settings'>('users');

    // ---- Users sub-tab ----
    let users = $state<CognitoUser[]>([]);
    let usersLoading = $state(false);
    let usersError = $state<string | null>(null);
    let selectedUser = $state<CognitoUserDetail | null>(null);
    let selectedUserLoading = $state(false);
    let showCreateUser = $state(false);
    let newUsername = $state('');
    let newUserTempPassword = $state('');
    let newUserEmail = $state('');
    let creatingUser = $state(false);
    let createUserError = $state<string | null>(null);
    let confirmDeleteUser = $state<string | null>(null);

    // User detail edit/actions
    let editingAttrs = $state(false);
    let editedAttrs = $state<{ name: string; value: string }[]>([]);
    let savingAttrs = $state(false);
    let setPasswordMode = $state(false);
    let newPasswordValue = $state('');
    let newPasswordPermanent = $state(true);
    let settingPassword = $state(false);
    let userActionError = $state<string | null>(null);
    let userGroups = $state<CognitoGroup[]>([]);
    let addingToGroup = $state(false);
    let addGroupName = $state('');
    let showAddGroup = $state(false);

    // ---- Groups sub-tab ----
    let groups = $state<CognitoGroup[]>([]);
    let groupsLoading = $state(false);
    let groupsError = $state<string | null>(null);
    let selectedGroup = $state<CognitoGroup | null>(null);
    let groupMembers = $state<CognitoUser[]>([]);
    let groupMembersLoading = $state(false);
    let showCreateGroup = $state(false);
    let newGroupName = $state('');
    let newGroupDescription = $state('');
    let newGroupRoleArn = $state('');
    let creatingGroup = $state(false);
    let createGroupError = $state<string | null>(null);
    let confirmDeleteGroup = $state<string | null>(null);

    // ---- Clients sub-tab ----
    let clients = $state<CognitoUserPoolClient[]>([]);
    let clientsLoading = $state(false);
    let clientsError = $state<string | null>(null);
    let selectedClient = $state<CognitoUserPoolClientDetail | null>(null);
    let selectedClientLoading = $state(false);
    let showClientSecret = $state(false);
    let showCreateClient = $state(false);
    let newClientName = $state('');
    let newClientGenerateSecret = $state(false);
    let newClientAuthFlows = $state<string[]>(['ALLOW_USER_PASSWORD_AUTH', 'ALLOW_REFRESH_TOKEN_AUTH']);
    let creatingClient = $state(false);
    let createClientError = $state<string | null>(null);
    let confirmDeleteClient = $state<string | null>(null);

    const ALL_AUTH_FLOWS = [
        'ALLOW_USER_PASSWORD_AUTH',
        'ALLOW_ADMIN_USER_PASSWORD_AUTH',
        'ALLOW_CUSTOM_AUTH',
        'ALLOW_USER_SRP_AUTH',
        'ALLOW_REFRESH_TOKEN_AUTH',
    ];

    // ---- Settings sub-tab ----
    let poolDetail = $state<unknown>(null);
    let poolDetailLoading = $state(false);

    // ---- Identity Pools ----
    let identityPools = $state<CognitoIdentityPool[]>([]);
    let identityPoolsLoading = $state(true);
    let identityPoolsError = $state<string | null>(null);
    let selectedIdentityPool = $state<CognitoIdentityPool | null>(null);
    let identityPoolDetail = $state<unknown>(null);
    let identityPoolDetailLoading = $state(false);
    let showCreateIdentityPool = $state(false);
    let newIdentityPoolName = $state('');
    let newIdentityPoolAllowUnauth = $state(false);
    let creatingIdentityPool = $state(false);
    let createIdentityPoolError = $state<string | null>(null);
    let confirmDeleteIdentityPool = $state<string | null>(null);

    // ---- Auth Tester ----
    let authPoolId = $state('');
    let authClientId = $state('');
    let authClients = $state<CognitoUserPoolClient[]>([]);

    // Sign Up
    let signUpUsername = $state('');
    let signUpPassword = $state('');
    let signUpEmail = $state('');
    let signUpResult = $state<unknown>(null);
    let signUpError = $state<string | null>(null);
    let signingUp = $state(false);
    let signUpSuccess = $state(false);

    // Sign In
    let signInUsername = $state('');
    let signInPassword = $state('');
    let signInError = $state<string | null>(null);
    let signingIn = $state(false);
    let signInSuccess = $state(false);
    let authTokens = $state<{ accessToken?: string; idToken?: string; refreshToken?: string; expiresIn?: number } | null>(null);
    let tokenExpiry = $state<Date | null>(null);
    let showIdToken = $state(false);
    let showAccessToken = $state(false);

    // GetUser test
    let getUserResult = $state<unknown>(null);
    let getUserError = $state<string | null>(null);
    let gettingUser = $state(false);

    // ---- Helpers ----
    function formatDate(iso: string): string {
        if (!iso) return '—';
        try { return new Date(iso).toLocaleString(); } catch { return iso; }
    }

    function statusBadge(status: string): string {
        switch (status) {
            case 'CONFIRMED': return 'bg-green-900/30 text-green-400';
            case 'UNCONFIRMED': return 'bg-yellow-900/30 text-yellow-400';
            case 'FORCE_CHANGE_PASSWORD': return 'bg-orange-900/30 text-orange-400';
            case 'DISABLED': return 'bg-red-900/30 text-red-400';
            default: return 'bg-zinc-700 text-zinc-400';
        }
    }

    function decodeJwt(token: string): { header: unknown; payload: unknown } | null {
        try {
            const parts = token.split('.');
            const header = JSON.parse(atob(parts[0].replace(/-/g, '+').replace(/_/g, '/')));
            const payload = JSON.parse(atob(parts[1].replace(/-/g, '+').replace(/_/g, '/')));
            return { header, payload };
        } catch {
            return null;
        }
    }

    function expiryCountdown(exp: Date): string {
        const diff = Math.floor((exp.getTime() - Date.now()) / 1000);
        if (diff <= 0) return 'Expired';
        const m = Math.floor(diff / 60);
        const s = diff % 60;
        return `${m}m ${s}s`;
    }

    // ---- User Pools ----
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
            await createUserPool(newPoolName.trim(), { mfaConfig: newPoolMfa });
            newPoolName = '';
            newPoolMfa = 'OFF';
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
        poolSubTab = 'users';
        selectedUser = null;
        selectedGroup = null;
        selectedClient = null;
        await loadPoolUsers(pool.id);
    }

    // ---- Users ----
    async function loadPoolUsers(poolId: string) {
        usersLoading = true;
        usersError = null;
        users = [];
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
            await adminCreateUser(selectedPool.id, newUsername.trim(), {
                tempPassword: newUserTempPassword || undefined,
                email: newUserEmail || undefined,
            });
            newUsername = '';
            newUserTempPassword = '';
            newUserEmail = '';
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
            if (selectedUser?.username === username) selectedUser = null;
            await loadPoolUsers(selectedPool.id);
        } catch (e) {
            usersError = e instanceof Error ? e.message : 'Failed to delete user';
        }
    }

    async function selectUser(username: string) {
        if (!selectedPool) return;
        selectedUserLoading = true;
        userActionError = null;
        editingAttrs = false;
        setPasswordMode = false;
        showAddGroup = false;
        try {
            const [detail, groupsData] = await Promise.all([
                adminGetUser(selectedPool.id, username),
                adminListGroupsForUser(selectedPool.id, username),
            ]);
            selectedUser = detail;
            editedAttrs = detail.attributes.map((a) => ({ ...a }));
            userGroups = groupsData.groups;
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to load user';
        } finally {
            selectedUserLoading = false;
        }
    }

    async function handleSaveAttrs() {
        if (!selectedPool || !selectedUser) return;
        savingAttrs = true;
        userActionError = null;
        try {
            await adminUpdateUserAttributes(selectedPool.id, selectedUser.username,
                editedAttrs.map((a) => ({ Name: a.name, Value: a.value })));
            selectedUser = { ...selectedUser, attributes: [...editedAttrs] };
            editingAttrs = false;
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to update attributes';
        } finally {
            savingAttrs = false;
        }
    }

    async function handleEnableUser() {
        if (!selectedPool || !selectedUser) return;
        userActionError = null;
        try {
            await adminEnableUser(selectedPool.id, selectedUser.username);
            selectedUser = { ...selectedUser, enabled: true };
            await loadPoolUsers(selectedPool.id);
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to enable user';
        }
    }

    async function handleDisableUser() {
        if (!selectedPool || !selectedUser) return;
        userActionError = null;
        try {
            await adminDisableUser(selectedPool.id, selectedUser.username);
            selectedUser = { ...selectedUser, enabled: false };
            await loadPoolUsers(selectedPool.id);
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to disable user';
        }
    }

    async function handleSetPassword() {
        if (!selectedPool || !selectedUser || !newPasswordValue) return;
        settingPassword = true;
        userActionError = null;
        try {
            await adminSetUserPassword(selectedPool.id, selectedUser.username, newPasswordValue, newPasswordPermanent);
            newPasswordValue = '';
            setPasswordMode = false;
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to set password';
        } finally {
            settingPassword = false;
        }
    }

    async function handleAddUserToGroup() {
        if (!selectedPool || !selectedUser || !addGroupName.trim()) return;
        addingToGroup = true;
        userActionError = null;
        try {
            await adminAddUserToGroup(selectedPool.id, selectedUser.username, addGroupName.trim());
            addGroupName = '';
            showAddGroup = false;
            const groupsData = await adminListGroupsForUser(selectedPool.id, selectedUser.username);
            userGroups = groupsData.groups;
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to add to group';
        } finally {
            addingToGroup = false;
        }
    }

    async function handleRemoveFromGroup(groupName: string) {
        if (!selectedPool || !selectedUser) return;
        userActionError = null;
        try {
            await adminRemoveUserFromGroup(selectedPool.id, selectedUser.username, groupName);
            userGroups = userGroups.filter((g) => g.name !== groupName);
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to remove from group';
        }
    }

    // ---- Groups ----
    async function loadGroups() {
        if (!selectedPool) return;
        groupsLoading = true;
        groupsError = null;
        groups = [];
        try {
            const data = await listCognitoGroups(selectedPool.id);
            groups = data.groups;
        } catch (e) {
            groupsError = e instanceof Error ? e.message : 'Failed to load groups';
        } finally {
            groupsLoading = false;
        }
    }

    async function handleCreateGroup() {
        if (!selectedPool || !newGroupName.trim()) return;
        creatingGroup = true;
        createGroupError = null;
        try {
            await createCognitoGroup(selectedPool.id, newGroupName.trim(), {
                description: newGroupDescription || undefined,
                roleArn: newGroupRoleArn || undefined,
            });
            newGroupName = '';
            newGroupDescription = '';
            newGroupRoleArn = '';
            showCreateGroup = false;
            await loadGroups();
        } catch (e) {
            createGroupError = e instanceof Error ? e.message : 'Failed to create group';
        } finally {
            creatingGroup = false;
        }
    }

    async function handleDeleteGroup(name: string) {
        if (!selectedPool) return;
        try {
            await deleteCognitoGroup(selectedPool.id, name);
            confirmDeleteGroup = null;
            if (selectedGroup?.name === name) selectedGroup = null;
            await loadGroups();
        } catch (e) {
            groupsError = e instanceof Error ? e.message : 'Failed to delete group';
        }
    }

    async function selectGroup(group: CognitoGroup) {
        if (!selectedPool) return;
        selectedGroup = group;
        groupMembersLoading = true;
        groupMembers = [];
        try {
            const data = await listUsersInGroup(selectedPool.id, group.name);
            groupMembers = data.users;
        } catch {
            groupMembers = [];
        } finally {
            groupMembersLoading = false;
        }
    }

    // ---- Clients ----
    async function loadClients() {
        if (!selectedPool) return;
        clientsLoading = true;
        clientsError = null;
        clients = [];
        try {
            const data = await listUserPoolClients(selectedPool.id);
            clients = data.clients;
        } catch (e) {
            clientsError = e instanceof Error ? e.message : 'Failed to load clients';
        } finally {
            clientsLoading = false;
        }
    }

    async function handleCreateClient() {
        if (!selectedPool || !newClientName.trim()) return;
        creatingClient = true;
        createClientError = null;
        try {
            await createUserPoolClient(selectedPool.id, newClientName.trim(), {
                generateSecret: newClientGenerateSecret,
                authFlows: newClientAuthFlows,
            });
            newClientName = '';
            newClientGenerateSecret = false;
            newClientAuthFlows = ['ALLOW_USER_PASSWORD_AUTH', 'ALLOW_REFRESH_TOKEN_AUTH'];
            showCreateClient = false;
            await loadClients();
        } catch (e) {
            createClientError = e instanceof Error ? e.message : 'Failed to create client';
        } finally {
            creatingClient = false;
        }
    }

    async function handleDeleteClient(clientId: string) {
        if (!selectedPool) return;
        try {
            await deleteUserPoolClient(selectedPool.id, clientId);
            confirmDeleteClient = null;
            if (selectedClient?.clientId === clientId) selectedClient = null;
            await loadClients();
        } catch (e) {
            clientsError = e instanceof Error ? e.message : 'Failed to delete client';
        }
    }

    async function selectClient(client: CognitoUserPoolClient) {
        if (!selectedPool) return;
        selectedClientLoading = true;
        showClientSecret = false;
        try {
            const detail = await describeUserPoolClient(selectedPool.id, client.clientId);
            selectedClient = detail;
        } catch {
            selectedClient = null;
        } finally {
            selectedClientLoading = false;
        }
    }

    function toggleAuthFlow(flow: string) {
        if (newClientAuthFlows.includes(flow)) {
            newClientAuthFlows = newClientAuthFlows.filter((f) => f !== flow);
        } else {
            newClientAuthFlows = [...newClientAuthFlows, flow];
        }
    }

    // ---- Settings ----
    async function loadPoolDetail() {
        if (!selectedPool) return;
        poolDetailLoading = true;
        try {
            const data = await describeUserPool(selectedPool.id);
            poolDetail = data;
        } catch {
            poolDetail = null;
        } finally {
            poolDetailLoading = false;
        }
    }

    // ---- Sub-tab switch ----
    async function switchPoolSubTab(tab: 'users' | 'groups' | 'clients' | 'settings') {
        poolSubTab = tab;
        selectedUser = null;
        selectedGroup = null;
        selectedClient = null;
        if (!selectedPool) return;
        if (tab === 'users' && users.length === 0 && !usersLoading) await loadPoolUsers(selectedPool.id);
        if (tab === 'groups') await loadGroups();
        if (tab === 'clients') await loadClients();
        if (tab === 'settings') await loadPoolDetail();
    }

    // ---- Identity Pools ----
    async function loadIdentityPools() {
        identityPoolsLoading = true;
        identityPoolsError = null;
        try {
            const data = await listIdentityPools();
            identityPools = data.identityPools;
        } catch {
            identityPoolsError = 'Could not load identity pools.';
        } finally {
            identityPoolsLoading = false;
        }
    }

    async function handleCreateIdentityPool() {
        if (!newIdentityPoolName.trim()) return;
        creatingIdentityPool = true;
        createIdentityPoolError = null;
        try {
            await createIdentityPool(newIdentityPoolName.trim(), newIdentityPoolAllowUnauth);
            newIdentityPoolName = '';
            newIdentityPoolAllowUnauth = false;
            showCreateIdentityPool = false;
            await loadIdentityPools();
        } catch (e) {
            createIdentityPoolError = e instanceof Error ? e.message : 'Failed to create identity pool';
        } finally {
            creatingIdentityPool = false;
        }
    }

    async function handleDeleteIdentityPool(id: string) {
        try {
            await deleteIdentityPool(id);
            confirmDeleteIdentityPool = null;
            if (selectedIdentityPool?.id === id) selectedIdentityPool = null;
            await loadIdentityPools();
        } catch (e) {
            identityPoolsError = e instanceof Error ? e.message : 'Failed to delete identity pool';
        }
    }

    async function selectIdentityPool(pool: CognitoIdentityPool) {
        selectedIdentityPool = pool;
        identityPoolDetailLoading = true;
        identityPoolDetail = null;
        try {
            identityPoolDetail = await describeIdentityPool(pool.id);
        } catch {
            identityPoolDetail = null;
        } finally {
            identityPoolDetailLoading = false;
        }
    }

    // ---- Auth Tester ----
    async function loadAuthClients() {
        authClients = [];
        authClientId = '';
        if (!authPoolId) return;
        try {
            const data = await listUserPoolClients(authPoolId);
            authClients = data.clients;
            // Auto-select the first client if only one exists
            if (authClients.length === 1) {
                authClientId = authClients[0].clientId;
            }
        } catch {
            authClients = [];
        }
    }

    async function handleSignUp() {
        if (!authClientId || !signUpUsername || !signUpPassword) return;
        signingUp = true;
        signUpError = null;
        signUpResult = null;
        signUpSuccess = false;
        try {
            signUpResult = await cognitoSignUp(authClientId, signUpUsername, signUpPassword, signUpEmail || undefined);
            signUpSuccess = true;
            setTimeout(() => { signUpSuccess = false; }, 3000);
        } catch (e) {
            signUpError = e instanceof Error ? e.message : 'Sign up failed';
        } finally {
            signingUp = false;
        }
    }

    async function handleSignIn() {
        if (!authClientId || !signInUsername || !signInPassword) return;
        signingIn = true;
        signInError = null;
        authTokens = null;
        getUserResult = null;
        signInSuccess = false;
        try {
            const result = await cognitoInitiateAuth(authClientId, signInUsername, signInPassword);
            authTokens = result;
            signInSuccess = true;
            setTimeout(() => { signInSuccess = false; }, 3000);
            if (result.expiresIn) {
                tokenExpiry = new Date(Date.now() + result.expiresIn * 1000);
            }
        } catch (e) {
            signInError = e instanceof Error ? e.message : 'Sign in failed';
        } finally {
            signingIn = false;
        }
    }

    async function handleGetUser() {
        if (!authTokens?.accessToken) return;
        gettingUser = true;
        getUserError = null;
        getUserResult = null;
        try {
            getUserResult = await cognitoGetUser(authTokens.accessToken);
        } catch (e) {
            getUserError = e instanceof Error ? e.message : 'GetUser failed';
        } finally {
            gettingUser = false;
        }
    }

    onMount(() => {
        loadPools();
        if (topTab === 'identitypools') loadIdentityPools();
    });
</script>

<div class="p-6">
    <div class="mb-6">
        <h1 class="text-2xl font-bold">Cognito</h1>
        <p class="text-zinc-500 mt-1">User authentication, authorization, and identity management.</p>
    </div>

    <!-- Top-level tabs — segmented button style -->
    <div class="flex bg-zinc-900 rounded-lg p-1 border border-zinc-800 mb-6">
        {#each [
            { id: 'userpools', label: 'User Pools', count: pools.length },
            { id: 'identitypools', label: 'Identity Pools', count: identityPools.length },
            { id: 'authtester', label: 'Auth Tester', count: null },
        ] as tab}
            <button
                onclick={() => {
                    topTab = tab.id as typeof topTab;
                    if (tab.id === 'identitypools' && identityPools.length === 0) loadIdentityPools();
                }}
                class="flex-1 px-4 py-2 text-sm font-medium rounded-md transition-all focus:outline-none focus:ring-2 focus:ring-orange-500/50 active:scale-[0.98] {topTab === tab.id
                    ? 'bg-zinc-700 text-orange-400 shadow-sm'
                    : 'text-zinc-500 hover:text-zinc-300'}"
            >
                {tab.label}
                {#if tab.count !== null}
                    <span class="ml-1.5 text-xs px-1.5 py-0.5 rounded-full {topTab === tab.id ? 'bg-orange-500/20 text-orange-400' : 'bg-zinc-800 text-zinc-500'}">{tab.count}</span>
                {/if}
            </button>
        {/each}
    </div>

    <!-- ============================================================ -->
    <!-- USER POOLS TAB -->
    <!-- ============================================================ -->
    {#if topTab === 'userpools'}
        <div class="flex items-center justify-between mb-4">
            <span class="text-sm text-zinc-400">{pools.length} pool{pools.length !== 1 ? 's' : ''}</span>
            <button
                onclick={() => { showCreatePool = !showCreatePool; createPoolError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50"
            >
                Create User Pool
            </button>
        </div>

        {#if showCreatePool}
            <div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4 mb-4 shadow-lg shadow-black/20">
                <h3 class="font-semibold mb-3 flex items-center gap-2">
                    <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                    Create User Pool
                </h3>
                {#if createPoolError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createPoolError}</div>
                {/if}
                <div class="grid grid-cols-2 gap-3 mb-3">
                    <div>
                        <label class="block text-xs text-zinc-400 mb-1">Pool Name</label>
                        <input
                            type="text"
                            bind:value={newPoolName}
                            onkeydown={(e) => e.key === 'Enter' && handleCreatePool()}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200"
                            placeholder="my-user-pool"
                        />
                    </div>
                    <div>
                        <label class="block text-xs text-zinc-400 mb-1">MFA Configuration</label>
                        <select
                            bind:value={newPoolMfa}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200 [&>option]:bg-zinc-800 [&>option]:text-zinc-200"
                        >
                            <option value="OFF">OFF</option>
                            <option value="OPTIONAL">OPTIONAL</option>
                            <option value="ON">ON</option>
                        </select>
                    </div>
                </div>
                <div class="flex gap-2">
                    <button
                        onclick={handleCreatePool}
                        disabled={creatingPool || !newPoolName.trim()}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50"
                    >
                        {creatingPool ? 'Creating...' : 'Create'}
                    </button>
                    <button
                        onclick={() => { showCreatePool = false; createPoolError = null; newPoolName = ''; }}
                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        {/if}

        {#if poolsLoading}
            <div class="text-zinc-500 text-sm">Loading...</div>
        {:else if poolsError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{poolsError}</div>
        {:else if pools.length === 0}
            <div class="bg-gradient-to-br from-zinc-900 to-zinc-950 rounded-lg border border-zinc-800 p-12 text-center shadow-lg shadow-black/20">
                <div class="text-4xl mb-3 opacity-30">🔑</div>
                <p class="text-zinc-500 mb-1">No user pools yet</p>
                <p class="text-zinc-600 text-sm mb-4">Create a user pool to manage authentication</p>
                <button
                    onclick={() => showCreatePool = true}
                    class="px-4 py-2 bg-orange-600 hover:bg-orange-500 rounded-lg text-sm font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50"
                >
                    Create User Pool
                </button>
            </div>
        {:else}
            <div class="flex gap-4">
                <!-- Pool list -->
                <div class="w-72 shrink-0">
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden shadow-lg shadow-black/20">
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
                                        class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs shrink-0 transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-red-500/40"
                                    >
                                        Delete
                                    </button>
                                </div>
                                {#if confirmDeletePool === pool.id}
                                    <div class="px-4 pb-3 bg-red-950/30 border-t border-red-900/30 backdrop-blur">
                                        <p class="text-xs text-red-400 mb-2">Delete "{pool.name}"?</p>
                                        <div class="flex gap-2">
                                            <button onclick={() => handleDeletePool(pool.id)} class="px-2 py-1 bg-red-700 hover:bg-red-600 rounded text-xs font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-red-500/40">Confirm</button>
                                            <button onclick={() => confirmDeletePool = null} class="px-2 py-1 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">Cancel</button>
                                        </div>
                                    </div>
                                {/if}
                            </div>
                        {/each}
                    </div>
                </div>

                <!-- Pool detail -->
                <div class="flex-1 min-w-0">
                    {#if selectedPool}
                        <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden shadow-lg shadow-black/20">
                            <!-- Pool header -->
                            <div class="px-4 py-3 border-b border-zinc-800">
                                <h2 class="font-semibold text-orange-400 font-mono">{selectedPool.name}</h2>
                                <div class="text-xs text-zinc-500 font-mono">{selectedPool.id}</div>
                            </div>

                            <!-- Pool sub-tabs -->
                            <div class="flex gap-1 px-2 border-b border-zinc-800 bg-zinc-900/50">
                                {#each [
                                    { id: 'users', label: 'Users', count: users.length },
                                    { id: 'groups', label: 'Groups', count: groups.length },
                                    { id: 'clients', label: 'App Clients', count: clients.length },
                                    { id: 'settings', label: 'Settings', count: null },
                                ] as tab}
                                    <button
                                        onclick={() => switchPoolSubTab(tab.id as typeof poolSubTab)}
                                        class="px-4 py-2.5 text-sm font-medium border-b-2 transition-all active:scale-[0.98] focus:outline-none {poolSubTab === tab.id
                                            ? 'border-orange-400 text-orange-400'
                                            : 'border-transparent text-zinc-500 hover:text-zinc-300 hover:border-zinc-600'}"
                                    >
                                        {tab.label}
                                        {#if tab.count !== null}
                                            <span class="ml-1 text-xs text-zinc-600">({tab.count})</span>
                                        {/if}
                                    </button>
                                {/each}
                            </div>

                            <!-- ---- Users sub-tab ---- -->
                            {#if poolSubTab === 'users'}
                                <div class="flex h-full">
                                    <div class="{selectedUser ? 'w-1/2' : 'w-full'} border-r border-zinc-800/50">
                                        <div class="px-4 py-3 border-b border-zinc-800 flex items-center justify-between">
                                            <span class="text-sm font-medium flex items-center gap-2">
                                                <span class="w-1.5 h-1.5 rounded-full bg-orange-500"></span>
                                                Users ({users.length})
                                            </span>
                                            <button
                                                onclick={() => { showCreateUser = !showCreateUser; createUserError = null; }}
                                                class="px-3 py-1 bg-orange-600 hover:bg-orange-500 rounded text-xs font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50"
                                            >
                                                Create User
                                            </button>
                                        </div>

                                        {#if showCreateUser}
                                            <div class="px-4 py-3 border-b border-zinc-800 bg-zinc-800/30">
                                                <h4 class="text-sm font-medium mb-2">Create User</h4>
                                                {#if createUserError}
                                                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs mb-2">{createUserError}</div>
                                                {/if}
                                                <div class="grid grid-cols-1 gap-2 mb-2">
                                                    <input
                                                        type="text"
                                                        bind:value={newUsername}
                                                        class="bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200"
                                                        placeholder="Username *"
                                                    />
                                                    <input
                                                        type="password"
                                                        bind:value={newUserTempPassword}
                                                        class="bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200"
                                                        placeholder="Temporary password (optional)"
                                                    />
                                                    <input
                                                        type="email"
                                                        bind:value={newUserEmail}
                                                        class="bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200"
                                                        placeholder="Email (optional)"
                                                    />
                                                </div>
                                                <div class="flex gap-2">
                                                    <button
                                                        onclick={handleCreateUser}
                                                        disabled={creatingUser || !newUsername.trim()}
                                                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-xs font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50"
                                                    >
                                                        {creatingUser ? 'Creating...' : 'Create'}
                                                    </button>
                                                    <button
                                                        onclick={() => { showCreateUser = false; createUserError = null; newUsername = ''; newUserTempPassword = ''; newUserEmail = ''; }}
                                                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50"
                                                    >
                                                        Cancel
                                                    </button>
                                                </div>
                                            </div>
                                        {/if}

                                        {#if usersLoading}
                                            <div class="p-4 text-zinc-500 text-sm">Loading users...</div>
                                        {:else if usersError}
                                            <div class="p-4 text-red-400 text-sm">{usersError}</div>
                                        {:else if users.length === 0}
                                            <div class="p-8 text-center text-zinc-500 text-sm">No users in this pool.</div>
                                        {:else}
                                            <div class="overflow-auto">
                                                <table class="w-full text-sm">
                                                    <thead>
                                                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                                                            <th class="px-4 py-2 text-xs">Username</th>
                                                            <th class="px-4 py-2 text-xs">Status</th>
                                                            <th class="px-4 py-2 text-xs">Created</th>
                                                            <th class="px-4 py-2 text-xs"></th>
                                                        </tr>
                                                    </thead>
                                                    <tbody>
                                                        {#each users as user}
                                                            <tr
                                                                class="border-b border-zinc-800/50 cursor-pointer {selectedUser?.username === user.username ? 'bg-zinc-800' : 'hover:bg-zinc-800/30'}"
                                                                onclick={() => selectUser(user.username)}
                                                            >
                                                                <td class="px-4 py-2.5 font-mono text-orange-400 text-xs">{user.username}</td>
                                                                <td class="px-4 py-2.5">
                                                                    <span class="px-1.5 py-0.5 rounded text-xs font-medium {statusBadge(user.status)}">
                                                                        {user.status || 'UNKNOWN'}
                                                                    </span>
                                                                </td>
                                                                <td class="px-4 py-2.5 text-zinc-500 text-xs">{formatDate(user.createDate)}</td>
                                                                <td class="px-4 py-2.5" onclick={(e) => e.stopPropagation()}>
                                                                    {#if confirmDeleteUser === user.username}
                                                                        <div class="flex gap-1">
                                                                            <button onclick={() => handleDeleteUser(user.username)} class="px-1.5 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs transition-all active:scale-[0.98]">OK</button>
                                                                            <button onclick={() => confirmDeleteUser = null} class="px-1.5 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98]">No</button>
                                                                        </div>
                                                                    {:else}
                                                                        <button onclick={() => confirmDeleteUser = user.username} class="text-red-400 hover:text-red-300 text-xs transition-colors">Del</button>
                                                                    {/if}
                                                                </td>
                                                            </tr>
                                                        {/each}
                                                    </tbody>
                                                </table>
                                            </div>
                                        {/if}
                                    </div>

                                    <!-- User detail panel -->
                                    {#if selectedUser}
                                        <div class="w-1/2 overflow-auto">
                                            {#if selectedUserLoading}
                                                <div class="p-4 text-zinc-500 text-sm">Loading...</div>
                                            {:else}
                                                <div class="p-4">
                                                    <div class="flex items-center justify-between mb-3">
                                                        <div>
                                                            <h3 class="font-mono text-orange-400 font-semibold">{selectedUser.username}</h3>
                                                            <div class="flex items-center gap-2 mt-1">
                                                                <span class="px-1.5 py-0.5 rounded text-xs font-medium {statusBadge(selectedUser.status)}">{selectedUser.status}</span>
                                                                <span class="text-xs {selectedUser.enabled ? 'text-green-400' : 'text-red-400'}">{selectedUser.enabled ? 'Enabled' : 'Disabled'}</span>
                                                            </div>
                                                        </div>
                                                        <button onclick={() => selectedUser = null} class="text-zinc-500 hover:text-zinc-300 text-xs transition-colors focus:outline-none focus:ring-2 focus:ring-orange-500/50 rounded">Close</button>
                                                    </div>

                                                    {#if userActionError}
                                                        <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs mb-3">{userActionError}</div>
                                                    {/if}

                                                    <!-- Actions -->
                                                    <div class="flex flex-wrap gap-2 mb-4">
                                                        {#if selectedUser.enabled}
                                                            <button onclick={handleDisableUser} class="px-2 py-1 bg-yellow-900/30 text-yellow-400 hover:bg-yellow-900/50 rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-yellow-500/40">Disable</button>
                                                        {:else}
                                                            <button onclick={handleEnableUser} class="px-2 py-1 bg-green-900/30 text-green-400 hover:bg-green-900/50 rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-green-500/40">Enable</button>
                                                        {/if}
                                                        <button onclick={() => setPasswordMode = !setPasswordMode} class="px-2 py-1 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">Set Password</button>
                                                        <button onclick={() => editingAttrs = !editingAttrs} class="px-2 py-1 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">Edit Attrs</button>
                                                        <button onclick={() => confirmDeleteUser = selectedUser?.username ?? null} class="px-2 py-1 bg-red-900/30 text-red-400 hover:bg-red-900/50 rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-red-500/40">Delete</button>
                                                    </div>

                                                    {#if confirmDeleteUser === selectedUser.username}
                                                        <div class="bg-red-950/30 border border-red-900/30 rounded p-3 mb-4">
                                                            <p class="text-xs text-red-400 mb-2">Delete "{selectedUser.username}"?</p>
                                                            <div class="flex gap-2">
                                                                <button onclick={() => handleDeleteUser(selectedUser!.username)} class="px-2 py-1 bg-red-700 hover:bg-red-600 rounded text-xs font-medium transition-all active:scale-[0.98]">Confirm</button>
                                                                <button onclick={() => confirmDeleteUser = null} class="px-2 py-1 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98]">Cancel</button>
                                                            </div>
                                                        </div>
                                                    {/if}

                                                    {#if setPasswordMode}
                                                        <div class="bg-zinc-800 rounded p-3 mb-4">
                                                            <h4 class="text-xs font-medium mb-2 text-zinc-300">Set Password</h4>
                                                            <input
                                                                type="password"
                                                                bind:value={newPasswordValue}
                                                                class="w-full bg-zinc-900 border border-zinc-700 rounded px-2 py-1.5 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200 mb-2"
                                                                placeholder="New password"
                                                            />
                                                            <label class="flex items-center gap-2 text-xs text-zinc-400 mb-2 cursor-pointer">
                                                                <input type="checkbox" bind:checked={newPasswordPermanent} class="rounded" />
                                                                Permanent (not temporary)
                                                            </label>
                                                            <div class="flex gap-2">
                                                                <button onclick={handleSetPassword} disabled={settingPassword || !newPasswordValue} class="px-2 py-1 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-xs font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">
                                                                    {settingPassword ? 'Setting...' : 'Set'}
                                                                </button>
                                                                <button onclick={() => { setPasswordMode = false; newPasswordValue = ''; }} class="px-2 py-1 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">Cancel</button>
                                                            </div>
                                                        </div>
                                                    {/if}

                                                    <!-- Attributes -->
                                                    <div class="mb-4">
                                                        <div class="flex items-center justify-between mb-2">
                                                            <h4 class="text-xs font-medium text-zinc-400 uppercase tracking-wider">Attributes</h4>
                                                            {#if editingAttrs}
                                                                <div class="flex gap-2">
                                                                    <button onclick={handleSaveAttrs} disabled={savingAttrs} class="px-2 py-0.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">Save</button>
                                                                    <button onclick={() => { editingAttrs = false; editedAttrs = selectedUser?.attributes.map(a => ({...a})) ?? []; }} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">Cancel</button>
                                                                </div>
                                                            {/if}
                                                        </div>
                                                        {#if editingAttrs}
                                                            {#each editedAttrs as attr, i}
                                                                <div class="flex gap-2 mb-1.5">
                                                                    <input bind:value={editedAttrs[i].name} class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-xs focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200" placeholder="Name" />
                                                                    <input bind:value={editedAttrs[i].value} class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-xs focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200" placeholder="Value" />
                                                                </div>
                                                            {/each}
                                                            <button onclick={() => editedAttrs = [...editedAttrs, { name: '', value: '' }]} class="text-xs text-orange-400 hover:text-orange-300 transition-colors">+ Add attribute</button>
                                                        {:else}
                                                            <div class="bg-zinc-800 rounded overflow-hidden">
                                                                {#each selectedUser.attributes as attr}
                                                                    <div class="flex px-3 py-2 border-b border-zinc-700/50 last:border-0">
                                                                        <span class="text-xs text-zinc-400 w-32 shrink-0 font-mono">{attr.name}</span>
                                                                        <span class="text-xs text-zinc-200 font-mono truncate">{attr.value}</span>
                                                                    </div>
                                                                {:else}
                                                                    <div class="px-3 py-2 text-xs text-zinc-500">No attributes</div>
                                                                {/each}
                                                            </div>
                                                        {/if}
                                                    </div>

                                                    <!-- Groups -->
                                                    <div>
                                                        <div class="flex items-center justify-between mb-2">
                                                            <h4 class="text-xs font-medium text-zinc-400 uppercase tracking-wider">Groups</h4>
                                                            <button onclick={() => showAddGroup = !showAddGroup} class="text-xs text-orange-400 hover:text-orange-300 transition-colors">+ Add</button>
                                                        </div>
                                                        {#if showAddGroup}
                                                            <div class="flex gap-2 mb-2">
                                                                <input
                                                                    bind:value={addGroupName}
                                                                    class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-xs focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200"
                                                                    placeholder="Group name"
                                                                />
                                                                <button onclick={handleAddUserToGroup} disabled={addingToGroup || !addGroupName.trim()} class="px-2 py-1 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">
                                                                    {addingToGroup ? '...' : 'Add'}
                                                                </button>
                                                                <button onclick={() => { showAddGroup = false; addGroupName = ''; }} class="px-2 py-1 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">Cancel</button>
                                                            </div>
                                                        {/if}
                                                        <div class="bg-zinc-800 rounded overflow-hidden">
                                                            {#each userGroups as g}
                                                                <div class="flex items-center justify-between px-3 py-2 border-b border-zinc-700/50 last:border-0">
                                                                    <span class="text-xs font-mono text-zinc-200">{g.name}</span>
                                                                    <button onclick={() => handleRemoveFromGroup(g.name)} class="text-xs text-red-400 hover:text-red-300 transition-colors">Remove</button>
                                                                </div>
                                                            {:else}
                                                                <div class="px-3 py-2 text-xs text-zinc-500">Not in any groups</div>
                                                            {/each}
                                                        </div>
                                                    </div>
                                                </div>
                                            {/if}
                                        </div>
                                    {/if}
                                </div>

                            <!-- ---- Groups sub-tab ---- -->
                            {:else if poolSubTab === 'groups'}
                                <div class="flex">
                                    <div class="{selectedGroup ? 'w-1/2' : 'w-full'} border-r border-zinc-800/50">
                                        <div class="px-4 py-3 border-b border-zinc-800 flex items-center justify-between">
                                            <span class="text-sm font-medium flex items-center gap-2">
                                                <span class="w-1.5 h-1.5 rounded-full bg-orange-500"></span>
                                                Groups ({groups.length})
                                            </span>
                                            <button
                                                onclick={() => { showCreateGroup = !showCreateGroup; createGroupError = null; }}
                                                class="px-3 py-1 bg-orange-600 hover:bg-orange-500 rounded text-xs font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50"
                                            >
                                                Create Group
                                            </button>
                                        </div>

                                        {#if showCreateGroup}
                                            <div class="px-4 py-3 border-b border-zinc-800 bg-zinc-800/30">
                                                {#if createGroupError}
                                                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs mb-2">{createGroupError}</div>
                                                {/if}
                                                <div class="grid grid-cols-1 gap-2 mb-2">
                                                    <input bind:value={newGroupName} class="bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200" placeholder="Group name *" />
                                                    <input bind:value={newGroupDescription} class="bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200" placeholder="Description (optional)" />
                                                    <input bind:value={newGroupRoleArn} class="bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm font-mono focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200" placeholder="Role ARN (optional)" />
                                                </div>
                                                <div class="flex gap-2">
                                                    <button onclick={handleCreateGroup} disabled={creatingGroup || !newGroupName.trim()} class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-xs font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">{creatingGroup ? 'Creating...' : 'Create'}</button>
                                                    <button onclick={() => { showCreateGroup = false; createGroupError = null; newGroupName = ''; newGroupDescription = ''; newGroupRoleArn = ''; }} class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">Cancel</button>
                                                </div>
                                            </div>
                                        {/if}

                                        {#if groupsLoading}
                                            <div class="p-4 text-zinc-500 text-sm">Loading groups...</div>
                                        {:else if groupsError}
                                            <div class="p-4 text-red-400 text-sm">{groupsError}</div>
                                        {:else if groups.length === 0}
                                            <div class="p-8 text-center text-zinc-500 text-sm">No groups yet.</div>
                                        {:else}
                                            <table class="w-full text-sm">
                                                <thead>
                                                    <tr class="border-b border-zinc-800 text-left text-zinc-500">
                                                        <th class="px-4 py-2 text-xs">Group Name</th>
                                                        <th class="px-4 py-2 text-xs">Description</th>
                                                        <th class="px-4 py-2 text-xs"></th>
                                                    </tr>
                                                </thead>
                                                <tbody>
                                                    {#each groups as group}
                                                        <tr class="border-b border-zinc-800/50 cursor-pointer {selectedGroup?.name === group.name ? 'bg-zinc-800' : 'hover:bg-zinc-800/30'}" onclick={() => selectGroup(group)}>
                                                            <td class="px-4 py-2.5 font-mono text-orange-400 text-xs">{group.name}</td>
                                                            <td class="px-4 py-2.5 text-zinc-400 text-xs">{group.description || '—'}</td>
                                                            <td class="px-4 py-2.5" onclick={(e) => e.stopPropagation()}>
                                                                {#if confirmDeleteGroup === group.name}
                                                                    <div class="flex gap-1">
                                                                        <button onclick={() => handleDeleteGroup(group.name)} class="px-1.5 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs transition-all active:scale-[0.98]">OK</button>
                                                                        <button onclick={() => confirmDeleteGroup = null} class="px-1.5 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98]">No</button>
                                                                    </div>
                                                                {:else}
                                                                    <button onclick={() => confirmDeleteGroup = group.name} class="text-red-400 hover:text-red-300 text-xs transition-colors">Del</button>
                                                                {/if}
                                                            </td>
                                                        </tr>
                                                    {/each}
                                                </tbody>
                                            </table>
                                        {/if}
                                    </div>

                                    {#if selectedGroup}
                                        <div class="w-1/2 p-4">
                                            <div class="flex items-center justify-between mb-3">
                                                <h3 class="font-mono text-orange-400 font-semibold">{selectedGroup.name}</h3>
                                                <button onclick={() => selectedGroup = null} class="text-zinc-500 hover:text-zinc-300 text-xs transition-colors focus:outline-none focus:ring-2 focus:ring-orange-500/50 rounded">Close</button>
                                            </div>
                                            {#if selectedGroup.description}
                                                <p class="text-sm text-zinc-400 mb-3">{selectedGroup.description}</p>
                                            {/if}
                                            {#if selectedGroup.roleArn}
                                                <div class="mb-3">
                                                    <span class="text-xs text-zinc-500">Role ARN: </span>
                                                    <span class="text-xs font-mono text-zinc-300">{selectedGroup.roleArn}</span>
                                                </div>
                                            {/if}
                                            <h4 class="text-xs font-medium text-zinc-400 uppercase tracking-wider mb-2">Members</h4>
                                            {#if groupMembersLoading}
                                                <div class="text-zinc-500 text-sm">Loading...</div>
                                            {:else if groupMembers.length === 0}
                                                <div class="text-zinc-500 text-sm">No members</div>
                                            {:else}
                                                <div class="bg-zinc-800 rounded overflow-hidden">
                                                    {#each groupMembers as member}
                                                        <div class="flex items-center justify-between px-3 py-2 border-b border-zinc-700/50 last:border-0">
                                                            <span class="text-xs font-mono text-orange-400">{member.username}</span>
                                                            <span class="px-1.5 py-0.5 rounded text-xs {statusBadge(member.status)}">{member.status}</span>
                                                        </div>
                                                    {/each}
                                                </div>
                                            {/if}
                                        </div>
                                    {/if}
                                </div>

                            <!-- ---- Clients sub-tab ---- -->
                            {:else if poolSubTab === 'clients'}
                                <div class="flex">
                                    <div class="{selectedClient ? 'w-1/2' : 'w-full'} border-r border-zinc-800/50">
                                        <div class="px-4 py-3 border-b border-zinc-800 flex items-center justify-between">
                                            <span class="text-sm font-medium flex items-center gap-2">
                                                <span class="w-1.5 h-1.5 rounded-full bg-orange-500"></span>
                                                App Clients ({clients.length})
                                            </span>
                                            <button
                                                onclick={() => { showCreateClient = !showCreateClient; createClientError = null; }}
                                                class="px-3 py-1 bg-orange-600 hover:bg-orange-500 rounded text-xs font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50"
                                            >
                                                Create Client
                                            </button>
                                        </div>

                                        {#if showCreateClient}
                                            <div class="px-4 py-3 border-b border-zinc-800 bg-zinc-800/30">
                                                {#if createClientError}
                                                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs mb-2">{createClientError}</div>
                                                {/if}
                                                <input bind:value={newClientName} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200 mb-2" placeholder="Client name *" />
                                                <label class="flex items-center gap-2 text-xs text-zinc-400 mb-3 cursor-pointer">
                                                    <input type="checkbox" bind:checked={newClientGenerateSecret} class="rounded" />
                                                    Generate client secret
                                                </label>
                                                <div class="mb-3">
                                                    <span class="text-xs text-zinc-400 block mb-1">Auth Flows</span>
                                                    <div class="flex flex-wrap gap-2">
                                                        {#each ALL_AUTH_FLOWS as flow}
                                                            <label class="flex items-center gap-1 cursor-pointer">
                                                                <input
                                                                    type="checkbox"
                                                                    checked={newClientAuthFlows.includes(flow)}
                                                                    onchange={() => toggleAuthFlow(flow)}
                                                                    class="rounded"
                                                                />
                                                                <span class="text-xs text-zinc-300">{flow.replace('ALLOW_', '')}</span>
                                                            </label>
                                                        {/each}
                                                    </div>
                                                </div>
                                                <div class="flex gap-2">
                                                    <button onclick={handleCreateClient} disabled={creatingClient || !newClientName.trim()} class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-xs font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">{creatingClient ? 'Creating...' : 'Create'}</button>
                                                    <button onclick={() => { showCreateClient = false; createClientError = null; newClientName = ''; }} class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">Cancel</button>
                                                </div>
                                            </div>
                                        {/if}

                                        {#if clientsLoading}
                                            <div class="p-4 text-zinc-500 text-sm">Loading clients...</div>
                                        {:else if clientsError}
                                            <div class="p-4 text-red-400 text-sm">{clientsError}</div>
                                        {:else if clients.length === 0}
                                            <div class="p-8 text-center text-zinc-500 text-sm">No app clients yet.</div>
                                        {:else}
                                            <table class="w-full text-sm">
                                                <thead>
                                                    <tr class="border-b border-zinc-800 text-left text-zinc-500">
                                                        <th class="px-4 py-2 text-xs">Client Name</th>
                                                        <th class="px-4 py-2 text-xs">Client ID</th>
                                                        <th class="px-4 py-2 text-xs"></th>
                                                    </tr>
                                                </thead>
                                                <tbody>
                                                    {#each clients as client}
                                                        <tr class="border-b border-zinc-800/50 cursor-pointer {selectedClient?.clientId === client.clientId ? 'bg-zinc-800' : 'hover:bg-zinc-800/30'}" onclick={() => selectClient(client)}>
                                                            <td class="px-4 py-2.5 text-zinc-200 text-sm">{client.clientName}</td>
                                                            <td class="px-4 py-2.5 font-mono text-zinc-400 text-xs">{client.clientId}</td>
                                                            <td class="px-4 py-2.5" onclick={(e) => e.stopPropagation()}>
                                                                {#if confirmDeleteClient === client.clientId}
                                                                    <div class="flex gap-1">
                                                                        <button onclick={() => handleDeleteClient(client.clientId)} class="px-1.5 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs transition-all active:scale-[0.98]">OK</button>
                                                                        <button onclick={() => confirmDeleteClient = null} class="px-1.5 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98]">No</button>
                                                                    </div>
                                                                {:else}
                                                                    <button onclick={() => confirmDeleteClient = client.clientId} class="text-red-400 hover:text-red-300 text-xs transition-colors">Del</button>
                                                                {/if}
                                                            </td>
                                                        </tr>
                                                    {/each}
                                                </tbody>
                                            </table>
                                        {/if}
                                    </div>

                                    {#if selectedClient}
                                        <div class="w-1/2 p-4 overflow-auto">
                                            {#if selectedClientLoading}
                                                <div class="text-zinc-500 text-sm">Loading...</div>
                                            {:else}
                                                <div class="flex items-center justify-between mb-3">
                                                    <h3 class="font-semibold text-zinc-200">{selectedClient.clientName}</h3>
                                                    <button onclick={() => selectedClient = null} class="text-zinc-500 hover:text-zinc-300 text-xs transition-colors focus:outline-none focus:ring-2 focus:ring-orange-500/50 rounded">Close</button>
                                                </div>
                                                <div class="space-y-3">
                                                    <div>
                                                        <span class="text-xs text-zinc-500">Client ID</span>
                                                        <div class="font-mono text-xs text-zinc-200 bg-zinc-800 rounded px-2 py-1 mt-0.5">{selectedClient.clientId}</div>
                                                    </div>
                                                    {#if selectedClient.clientSecret}
                                                        <div>
                                                            <span class="text-xs text-zinc-500">Client Secret</span>
                                                            <div class="flex items-center gap-2 mt-0.5">
                                                                <div class="font-mono text-xs text-zinc-200 bg-zinc-800 rounded px-2 py-1 flex-1">
                                                                    {showClientSecret ? selectedClient.clientSecret : '••••••••••••••••••••••••'}
                                                                </div>
                                                                <button onclick={() => showClientSecret = !showClientSecret} class="text-xs text-orange-400 hover:text-orange-300 transition-colors">{showClientSecret ? 'Hide' : 'Show'}</button>
                                                            </div>
                                                        </div>
                                                    {/if}
                                                    {#if selectedClient.explicitAuthFlows.length > 0}
                                                        <div>
                                                            <span class="text-xs text-zinc-500">Auth Flows</span>
                                                            <div class="flex flex-wrap gap-1 mt-1">
                                                                {#each selectedClient.explicitAuthFlows as flow}
                                                                    <span class="px-1.5 py-0.5 bg-zinc-700 rounded text-xs font-mono">{flow}</span>
                                                                {/each}
                                                            </div>
                                                        </div>
                                                    {/if}
                                                    {#if selectedClient.callbackUrLs.length > 0}
                                                        <div>
                                                            <span class="text-xs text-zinc-500">Callback URLs</span>
                                                            {#each selectedClient.callbackUrLs as url}
                                                                <div class="font-mono text-xs text-zinc-300 mt-0.5">{url}</div>
                                                            {/each}
                                                        </div>
                                                    {/if}
                                                    {#if selectedClient.allowedOAuthScopes.length > 0}
                                                        <div>
                                                            <span class="text-xs text-zinc-500">OAuth Scopes</span>
                                                            <div class="flex flex-wrap gap-1 mt-1">
                                                                {#each selectedClient.allowedOAuthScopes as scope}
                                                                    <span class="px-1.5 py-0.5 bg-zinc-700 rounded text-xs">{scope}</span>
                                                                {/each}
                                                            </div>
                                                        </div>
                                                    {/if}
                                                </div>
                                            {/if}
                                        </div>
                                    {/if}
                                </div>

                            <!-- ---- Settings sub-tab ---- -->
                            {:else if poolSubTab === 'settings'}
                                <div class="p-4">
                                    {#if poolDetailLoading}
                                        <div class="text-zinc-500 text-sm">Loading settings...</div>
                                    {:else if poolDetail}
                                        {@const pd = poolDetail as Record<string, unknown>}
                                        {@const up = (pd['UserPool'] ?? {}) as Record<string, unknown>}
                                        <div class="space-y-4">
                                            <div>
                                                <h4 class="text-xs font-medium text-zinc-400 uppercase tracking-wider mb-2">MFA Configuration</h4>
                                                <div class="bg-zinc-800 rounded px-3 py-2 text-sm text-zinc-200">
                                                    {String(up['MfaConfiguration'] ?? 'OFF')}
                                                </div>
                                            </div>
                                            {#if up['Policies']}
                                                {@const policies = (up['Policies'] as Record<string, unknown>)['PasswordPolicy'] as Record<string, unknown> | undefined}
                                                {#if policies}
                                                    <div>
                                                        <h4 class="text-xs font-medium text-zinc-400 uppercase tracking-wider mb-2">Password Policy</h4>
                                                        <div class="bg-zinc-800 rounded p-3 space-y-1">
                                                            <div class="flex justify-between text-xs">
                                                                <span class="text-zinc-400">Minimum Length</span>
                                                                <span class="text-zinc-200">{String(policies['MinimumLength'] ?? '8')}</span>
                                                            </div>
                                                            <div class="flex justify-between text-xs">
                                                                <span class="text-zinc-400">Require Uppercase</span>
                                                                <span class="{policies['RequireUppercase'] ? 'text-green-400' : 'text-zinc-500'}">{policies['RequireUppercase'] ? 'Yes' : 'No'}</span>
                                                            </div>
                                                            <div class="flex justify-between text-xs">
                                                                <span class="text-zinc-400">Require Lowercase</span>
                                                                <span class="{policies['RequireLowercase'] ? 'text-green-400' : 'text-zinc-500'}">{policies['RequireLowercase'] ? 'Yes' : 'No'}</span>
                                                            </div>
                                                            <div class="flex justify-between text-xs">
                                                                <span class="text-zinc-400">Require Numbers</span>
                                                                <span class="{policies['RequireNumbers'] ? 'text-green-400' : 'text-zinc-500'}">{policies['RequireNumbers'] ? 'Yes' : 'No'}</span>
                                                            </div>
                                                            <div class="flex justify-between text-xs">
                                                                <span class="text-zinc-400">Require Symbols</span>
                                                                <span class="{policies['RequireSymbols'] ? 'text-green-400' : 'text-zinc-500'}">{policies['RequireSymbols'] ? 'Yes' : 'No'}</span>
                                                            </div>
                                                        </div>
                                                    </div>
                                                {/if}
                                            {/if}
                                            {#if up['LambdaConfig']}
                                                {@const lambdaConfig = up['LambdaConfig'] as Record<string, string>}
                                                <div>
                                                    <h4 class="text-xs font-medium text-zinc-400 uppercase tracking-wider mb-2">Lambda Triggers</h4>
                                                    <div class="bg-zinc-800 rounded overflow-hidden">
                                                        {#each Object.entries(lambdaConfig) as [trigger, arn]}
                                                            <div class="flex px-3 py-2 border-b border-zinc-700/50 last:border-0">
                                                                <span class="text-xs text-zinc-400 w-40 shrink-0">{trigger}</span>
                                                                <span class="text-xs font-mono text-zinc-200 truncate">{arn}</span>
                                                            </div>
                                                        {/each}
                                                    </div>
                                                </div>
                                            {/if}
                                            {#if up['AutoVerifiedAttributes']}
                                                <div>
                                                    <h4 class="text-xs font-medium text-zinc-400 uppercase tracking-wider mb-2">Auto-Verified Attributes</h4>
                                                    <div class="flex flex-wrap gap-1">
                                                        {#each (up['AutoVerifiedAttributes'] as string[]) as attr}
                                                            <span class="px-2 py-0.5 bg-zinc-700 rounded text-xs">{attr}</span>
                                                        {/each}
                                                    </div>
                                                </div>
                                            {/if}
                                            <div>
                                                <h4 class="text-xs font-medium text-zinc-400 uppercase tracking-wider mb-2">Pool ID</h4>
                                                <div class="font-mono text-xs text-zinc-300 bg-zinc-800 rounded px-3 py-2">{String(up['Id'] ?? selectedPool.id)}</div>
                                            </div>
                                            <div>
                                                <h4 class="text-xs font-medium text-zinc-400 uppercase tracking-wider mb-2">Creation Date</h4>
                                                <div class="text-xs text-zinc-300 bg-zinc-800 rounded px-3 py-2">{up['CreationDate'] ? formatDate(new Date(Number(up['CreationDate']) * 1000).toISOString()) : '—'}</div>
                                            </div>
                                        </div>
                                    {:else}
                                        <div class="text-zinc-500 text-sm">No settings available.</div>
                                    {/if}
                                </div>
                            {/if}
                        </div>
                    {:else}
                        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-500 text-sm shadow-lg shadow-black/20">
                            Select a user pool to view details and manage users.
                        </div>
                    {/if}
                </div>
            </div>
        {/if}
    {/if}

    <!-- ============================================================ -->
    <!-- IDENTITY POOLS TAB -->
    <!-- ============================================================ -->
    {#if topTab === 'identitypools'}
        <div class="flex items-center justify-between mb-4">
            <span class="text-sm text-zinc-400">{identityPools.length} pool{identityPools.length !== 1 ? 's' : ''}</span>
            <button
                onclick={() => { showCreateIdentityPool = !showCreateIdentityPool; createIdentityPoolError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50"
            >
                Create Identity Pool
            </button>
        </div>

        {#if showCreateIdentityPool}
            <div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4 mb-4 shadow-lg shadow-black/20">
                <h3 class="font-semibold mb-3 flex items-center gap-2">
                    <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                    Create Identity Pool
                </h3>
                {#if createIdentityPoolError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createIdentityPoolError}</div>
                {/if}
                <input
                    type="text"
                    bind:value={newIdentityPoolName}
                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200 mb-3"
                    placeholder="Identity pool name"
                />
                <label class="flex items-center gap-2 text-sm text-zinc-400 mb-3 cursor-pointer">
                    <input type="checkbox" bind:checked={newIdentityPoolAllowUnauth} class="rounded" />
                    Allow unauthenticated identities
                </label>
                <div class="flex gap-2">
                    <button onclick={handleCreateIdentityPool} disabled={creatingIdentityPool || !newIdentityPoolName.trim()} class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">
                        {creatingIdentityPool ? 'Creating...' : 'Create'}
                    </button>
                    <button onclick={() => { showCreateIdentityPool = false; createIdentityPoolError = null; newIdentityPoolName = ''; }} class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">Cancel</button>
                </div>
            </div>
        {/if}

        {#if identityPoolsLoading}
            <div class="text-zinc-500 text-sm">Loading...</div>
        {:else if identityPoolsError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{identityPoolsError}</div>
        {:else if identityPools.length === 0}
            <div class="bg-gradient-to-br from-zinc-900 to-zinc-950 rounded-lg border border-zinc-800 p-12 text-center shadow-lg shadow-black/20">
                <div class="text-4xl mb-3 opacity-30">🪪</div>
                <p class="text-zinc-500 mb-1">No identity pools yet</p>
                <p class="text-zinc-600 text-sm mb-4">Create an identity pool for federated identity access</p>
                <button onclick={() => showCreateIdentityPool = true} class="px-4 py-2 bg-orange-600 hover:bg-orange-500 rounded-lg text-sm font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">Create your first identity pool</button>
            </div>
        {:else}
            <div class="flex gap-4">
                <div class="w-72 shrink-0">
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden shadow-lg shadow-black/20">
                        {#each identityPools as pool}
                            <div class="border-b border-zinc-800/50 last:border-0 {selectedIdentityPool?.id === pool.id ? 'bg-zinc-800' : 'hover:bg-zinc-800/40'} transition-colors">
                                <div class="px-4 py-3 flex items-start justify-between gap-2">
                                    <button class="flex-1 text-left min-w-0" onclick={() => selectIdentityPool(pool)}>
                                        <div class="font-mono text-orange-400 text-sm truncate">{pool.name}</div>
                                        <div class="text-xs text-zinc-500 mt-0.5 font-mono truncate">{pool.id}</div>
                                        <div class="mt-1">
                                            <span class="text-xs px-1.5 py-0.5 rounded {pool.allowUnauthenticated ? 'bg-green-900/30 text-green-400' : 'bg-zinc-700 text-zinc-500'}">
                                                {pool.allowUnauthenticated ? 'Unauth allowed' : 'Auth only'}
                                            </span>
                                        </div>
                                    </button>
                                    <button onclick={(e) => { e.stopPropagation(); confirmDeleteIdentityPool = pool.id; }} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs shrink-0 transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-red-500/40">Delete</button>
                                </div>
                                {#if confirmDeleteIdentityPool === pool.id}
                                    <div class="px-4 pb-3 bg-red-950/30 border-t border-red-900/30 backdrop-blur">
                                        <p class="text-xs text-red-400 mb-2">Delete "{pool.name}"?</p>
                                        <div class="flex gap-2">
                                            <button onclick={() => handleDeleteIdentityPool(pool.id)} class="px-2 py-1 bg-red-700 hover:bg-red-600 rounded text-xs font-medium transition-all active:scale-[0.98]">Confirm</button>
                                            <button onclick={() => confirmDeleteIdentityPool = null} class="px-2 py-1 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98]">Cancel</button>
                                        </div>
                                    </div>
                                {/if}
                            </div>
                        {/each}
                    </div>
                </div>

                <div class="flex-1 min-w-0">
                    {#if selectedIdentityPool}
                        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4 shadow-lg shadow-black/20">
                            <h2 class="font-semibold text-orange-400 font-mono mb-1">{selectedIdentityPool.name}</h2>
                            <div class="text-xs text-zinc-500 font-mono mb-4">{selectedIdentityPool.id}</div>
                            {#if identityPoolDetailLoading}
                                <div class="text-zinc-500 text-sm">Loading details...</div>
                            {:else if identityPoolDetail}
                                {@const d = identityPoolDetail as Record<string, unknown>}
                                <div class="space-y-4">
                                    <div>
                                        <h4 class="text-xs font-medium text-zinc-400 uppercase tracking-wider mb-2">Configuration</h4>
                                        <div class="bg-zinc-800 rounded p-3 space-y-2">
                                            <div class="flex justify-between text-xs">
                                                <span class="text-zinc-400">Allow Unauthenticated</span>
                                                <span class="{d['AllowUnauthenticatedIdentities'] ? 'text-green-400' : 'text-zinc-500'}">{d['AllowUnauthenticatedIdentities'] ? 'Yes' : 'No'}</span>
                                            </div>
                                            {#if d['AllowClassicFlow'] !== undefined}
                                                <div class="flex justify-between text-xs">
                                                    <span class="text-zinc-400">Classic Flow</span>
                                                    <span class="{d['AllowClassicFlow'] ? 'text-green-400' : 'text-zinc-500'}">{d['AllowClassicFlow'] ? 'Enabled' : 'Disabled'}</span>
                                                </div>
                                            {/if}
                                        </div>
                                    </div>
                                    {#if d['CognitoIdentityProviders']}
                                        <div>
                                            <h4 class="text-xs font-medium text-zinc-400 uppercase tracking-wider mb-2">Cognito Identity Providers</h4>
                                            <div class="bg-zinc-800 rounded overflow-hidden">
                                                {#each (d['CognitoIdentityProviders'] as Record<string, string>[]) as provider}
                                                    <div class="px-3 py-2 border-b border-zinc-700/50 last:border-0">
                                                        <div class="text-xs font-mono text-zinc-200">{provider['ProviderName'] ?? '—'}</div>
                                                        <div class="text-xs text-zinc-500 mt-0.5">Client: {provider['ClientId'] ?? '—'}</div>
                                                    </div>
                                                {:else}
                                                    <div class="px-3 py-2 text-xs text-zinc-500">No providers configured</div>
                                                {/each}
                                            </div>
                                        </div>
                                    {/if}
                                    {#if d['Roles']}
                                        <div>
                                            <h4 class="text-xs font-medium text-zinc-400 uppercase tracking-wider mb-2">Roles</h4>
                                            <div class="bg-zinc-800 rounded overflow-hidden">
                                                {#each Object.entries(d['Roles'] as Record<string, string>) as [role, arn]}
                                                    <div class="flex px-3 py-2 border-b border-zinc-700/50 last:border-0">
                                                        <span class="text-xs text-zinc-400 w-28 shrink-0 capitalize">{role}</span>
                                                        <span class="text-xs font-mono text-zinc-200 truncate">{arn}</span>
                                                    </div>
                                                {/each}
                                            </div>
                                        </div>
                                    {/if}
                                </div>
                            {/if}
                        </div>
                    {:else}
                        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-500 text-sm shadow-lg shadow-black/20">
                            Select an identity pool to view details.
                        </div>
                    {/if}
                </div>
            </div>
        {/if}
    {/if}

    <!-- ============================================================ -->
    <!-- AUTH TESTER TAB -->
    <!-- ============================================================ -->
    {#if topTab === 'authtester'}
        <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
            <!-- Left: Config + Sign Up/In -->
            <div class="space-y-4">
                <!-- Pool & Client selector -->
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4 shadow-lg shadow-black/20">
                    <h3 class="font-semibold mb-3 flex items-center gap-2">
                        <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                        Configuration
                    </h3>
                    <div class="space-y-3">
                        <div>
                            <label class="block text-xs text-zinc-400 mb-1">User Pool</label>
                            <select
                                bind:value={authPoolId}
                                onchange={loadAuthClients}
                                class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200 [&>option]:bg-zinc-800 [&>option]:text-zinc-200"
                            >
                                <option value="">Select a user pool...</option>
                                {#each pools as pool}
                                    <option value={pool.id}>{pool.name} ({pool.id})</option>
                                {/each}
                            </select>
                        </div>
                        <div>
                            <label class="block text-xs text-zinc-400 mb-1">App Client</label>
                            <select
                                bind:value={authClientId}
                                class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200 [&>option]:bg-zinc-800 [&>option]:text-zinc-200 disabled:opacity-40 disabled:cursor-not-allowed disabled:bg-zinc-900"
                                disabled={!authPoolId}
                            >
                                <option value="">Select a client...</option>
                                {#each authClients as client}
                                    <option value={client.clientId}>{client.clientName} ({client.clientId})</option>
                                {/each}
                            </select>
                        </div>
                    </div>
                </div>

                <!-- Sign Up -->
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4 shadow-lg shadow-black/20">
                    <h3 class="font-semibold mb-3 flex items-center gap-2">
                        <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                        Sign Up
                    </h3>
                    {#if signUpSuccess}
                        <div class="bg-green-900/30 border border-green-700/50 rounded p-2 text-green-400 text-xs mb-3 flex items-center gap-2">
                            <span class="w-1.5 h-1.5 rounded-full bg-green-400 shrink-0"></span>
                            Sign up successful!
                        </div>
                    {/if}
                    <div class="space-y-2 mb-3">
                        <input bind:value={signUpUsername} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200" placeholder="Username" />
                        <input type="password" bind:value={signUpPassword} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200" placeholder="Password" />
                        <input type="email" bind:value={signUpEmail} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200" placeholder="Email (optional)" />
                    </div>
                    {#if signUpError}
                        <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs mb-3">{signUpError}</div>
                    {/if}
                    <button
                        onclick={handleSignUp}
                        disabled={signingUp || !authClientId || !signUpUsername || !signUpPassword}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50"
                    >
                        {signingUp ? 'Signing Up...' : 'Sign Up'}
                    </button>
                    {#if signUpResult}
                        <div class="mt-3 bg-zinc-800 rounded p-3">
                            <span class="text-xs text-zinc-400 block mb-1">Result</span>
                            <pre class="text-xs text-green-400 overflow-auto">{JSON.stringify(signUpResult, null, 2)}</pre>
                        </div>
                    {/if}
                </div>

                <!-- Sign In -->
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4 shadow-lg shadow-black/20">
                    <h3 class="font-semibold mb-3 flex items-center gap-2">
                        <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                        Sign In
                    </h3>
                    {#if signInSuccess}
                        <div class="bg-green-900/30 border border-green-700/50 rounded p-2 text-green-400 text-xs mb-3 flex items-center gap-2">
                            <span class="w-1.5 h-1.5 rounded-full bg-green-400 shrink-0"></span>
                            Signed in successfully!
                        </div>
                    {/if}
                    <div class="space-y-2 mb-3">
                        <input bind:value={signInUsername} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200" placeholder="Username" />
                        <input type="password" bind:value={signInPassword} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200" placeholder="Password" />
                    </div>
                    {#if signInError}
                        <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs mb-3">{signInError}</div>
                    {/if}
                    <button
                        onclick={handleSignIn}
                        disabled={signingIn || !authClientId || !signInUsername || !signInPassword}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50"
                    >
                        {signingIn ? 'Signing In...' : 'Sign In'}
                    </button>

                    {#if authTokens}
                        <div class="mt-3 space-y-2">
                            {#if tokenExpiry}
                                <div class="text-xs text-zinc-400">Expires: <span class="text-orange-400">{expiryCountdown(tokenExpiry)}</span> ({tokenExpiry.toLocaleTimeString()})</div>
                            {/if}
                            {#if authTokens.refreshToken}
                                <div>
                                    <span class="text-xs text-zinc-500">Refresh Token (opaque)</span>
                                    <div class="font-mono text-xs text-zinc-400 bg-zinc-800 rounded px-2 py-1 mt-0.5 truncate">{authTokens.refreshToken}</div>
                                </div>
                            {/if}
                        </div>
                    {/if}
                </div>

                <!-- Test GetUser -->
                {#if authTokens?.accessToken}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4 shadow-lg shadow-black/20">
                        <div class="flex items-center justify-between mb-3">
                            <h3 class="font-semibold flex items-center gap-2">
                                <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                                Test GetUser
                            </h3>
                            <button onclick={handleGetUser} disabled={gettingUser} class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">
                                {gettingUser ? 'Loading...' : 'GetUser'}
                            </button>
                        </div>
                        {#if getUserError}
                            <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs">{getUserError}</div>
                        {/if}
                        {#if getUserResult}
                            <pre class="text-xs text-green-400 bg-zinc-800 rounded p-3 overflow-auto">{JSON.stringify(getUserResult, null, 2)}</pre>
                        {/if}
                    </div>
                {/if}
            </div>

            <!-- Right: Token Inspector -->
            <div class="space-y-4">
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4 shadow-lg shadow-black/20">
                    <h3 class="font-semibold mb-3 flex items-center gap-2">
                        <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                        Token Inspector
                    </h3>
                    {#if !authTokens}
                        <div class="text-zinc-500 text-sm">Sign in to inspect tokens.</div>
                    {:else}
                        <div class="space-y-4">
                            <!-- ID Token -->
                            {#if authTokens.idToken}
                                {@const decoded = decodeJwt(authTokens.idToken)}
                                <div class="border border-zinc-700 rounded-lg overflow-hidden">
                                    <div class="flex items-center justify-between px-3 py-2 bg-zinc-800 border-b border-zinc-700">
                                        <span class="text-sm font-medium text-zinc-200">ID Token</span>
                                        <button onclick={() => showIdToken = !showIdToken} class="text-xs text-orange-400 hover:text-orange-300 transition-colors">{showIdToken ? 'Hide raw' : 'Show raw'}</button>
                                    </div>
                                    {#if showIdToken}
                                        <div class="p-3">
                                            <div class="font-mono text-xs text-zinc-400 break-all">{authTokens.idToken}</div>
                                        </div>
                                    {/if}
                                    {#if decoded}
                                        <div class="p-3 space-y-3">
                                            {#if decoded.payload}
                                                {@const p = decoded.payload as Record<string, unknown>}
                                                <div class="grid grid-cols-2 gap-x-4 gap-y-1 text-xs mb-3 bg-zinc-800/50 rounded p-3">
                                                    {#if p.sub}<span class="text-zinc-500">Subject (sub)</span><span class="text-zinc-200 font-mono truncate">{String(p.sub)}</span>{/if}
                                                    {#if p.iss}<span class="text-zinc-500">Issuer (iss)</span><span class="text-zinc-200 font-mono truncate">{String(p.iss)}</span>{/if}
                                                    {#if p.token_use}<span class="text-zinc-500">Token Use</span><span class="text-zinc-200">{String(p.token_use)}</span>{/if}
                                                    {#if p.exp}<span class="text-zinc-500">Expires</span><span class="text-zinc-200">{new Date(Number(p.exp) * 1000).toLocaleString()}</span>{/if}
                                                    {#if p.scope}<span class="text-zinc-500">Scopes</span><span class="text-zinc-200">{String(p.scope)}</span>{/if}
                                                    {#if p['cognito:username']}<span class="text-zinc-500">Username</span><span class="text-zinc-200">{String(p['cognito:username'])}</span>{/if}
                                                    {#if p.email}<span class="text-zinc-500">Email</span><span class="text-zinc-200">{String(p.email)}</span>{/if}
                                                </div>
                                            {/if}
                                            <div>
                                                <span class="text-xs text-zinc-500 font-medium">Header</span>
                                                <pre class="text-xs text-blue-400 mt-1 bg-zinc-800 rounded p-2 overflow-auto">{JSON.stringify(decoded.header, null, 2)}</pre>
                                            </div>
                                            <div>
                                                <span class="text-xs text-zinc-500 font-medium">Payload</span>
                                                <pre class="text-xs text-green-400 mt-1 bg-zinc-800 rounded p-2 overflow-auto">{JSON.stringify(decoded.payload, null, 2)}</pre>
                                            </div>
                                        </div>
                                    {/if}
                                </div>
                            {/if}

                            <!-- Access Token -->
                            {#if authTokens.accessToken}
                                {@const decoded = decodeJwt(authTokens.accessToken)}
                                <div class="border border-zinc-700 rounded-lg overflow-hidden">
                                    <div class="flex items-center justify-between px-3 py-2 bg-zinc-800 border-b border-zinc-700">
                                        <span class="text-sm font-medium text-zinc-200">Access Token</span>
                                        <button onclick={() => showAccessToken = !showAccessToken} class="text-xs text-orange-400 hover:text-orange-300 transition-colors">{showAccessToken ? 'Hide raw' : 'Show raw'}</button>
                                    </div>
                                    {#if showAccessToken}
                                        <div class="p-3">
                                            <div class="font-mono text-xs text-zinc-400 break-all">{authTokens.accessToken}</div>
                                        </div>
                                    {/if}
                                    {#if decoded}
                                        <div class="p-3 space-y-3">
                                            {#if decoded.payload}
                                                {@const p = decoded.payload as Record<string, unknown>}
                                                <div class="grid grid-cols-2 gap-x-4 gap-y-1 text-xs mb-3 bg-zinc-800/50 rounded p-3">
                                                    {#if p.sub}<span class="text-zinc-500">Subject (sub)</span><span class="text-zinc-200 font-mono truncate">{String(p.sub)}</span>{/if}
                                                    {#if p.iss}<span class="text-zinc-500">Issuer (iss)</span><span class="text-zinc-200 font-mono truncate">{String(p.iss)}</span>{/if}
                                                    {#if p.token_use}<span class="text-zinc-500">Token Use</span><span class="text-zinc-200">{String(p.token_use)}</span>{/if}
                                                    {#if p.exp}<span class="text-zinc-500">Expires</span><span class="text-zinc-200">{new Date(Number(p.exp) * 1000).toLocaleString()}</span>{/if}
                                                    {#if p.scope}<span class="text-zinc-500">Scopes</span><span class="text-zinc-200">{String(p.scope)}</span>{/if}
                                                    {#if p['cognito:username']}<span class="text-zinc-500">Username</span><span class="text-zinc-200">{String(p['cognito:username'])}</span>{/if}
                                                    {#if p.client_id}<span class="text-zinc-500">Client ID</span><span class="text-zinc-200 font-mono truncate">{String(p.client_id)}</span>{/if}
                                                </div>
                                            {/if}
                                            <div>
                                                <span class="text-xs text-zinc-500 font-medium">Header</span>
                                                <pre class="text-xs text-blue-400 mt-1 bg-zinc-800 rounded p-2 overflow-auto">{JSON.stringify(decoded.header, null, 2)}</pre>
                                            </div>
                                            <div>
                                                <span class="text-xs text-zinc-500 font-medium">Payload</span>
                                                <pre class="text-xs text-green-400 mt-1 bg-zinc-800 rounded p-2 overflow-auto">{JSON.stringify(decoded.payload, null, 2)}</pre>
                                            </div>
                                        </div>
                                    {/if}
                                </div>
                            {/if}
                        </div>
                    {/if}
                </div>
            </div>
        </div>
    {/if}
</div>
