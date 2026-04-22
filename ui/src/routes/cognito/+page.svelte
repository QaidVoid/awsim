<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listUserPools, createUserPool, deleteUserPool, describeUserPool,
        updateUserPool,
        createUserPoolDomain, deleteUserPoolDomain,
        listResourceServers, createResourceServer, deleteResourceServer,
        listIdentityProviders, createIdentityProvider, deleteIdentityProvider,
        addCustomAttributes,
        listUserPoolClients, createUserPoolClient, describeUserPoolClient, deleteUserPoolClient,
        listCognitoUsers, adminCreateUser, adminDeleteUser, adminGetUser,
        adminSetUserPassword, adminEnableUser, adminDisableUser, adminUpdateUserAttributes,
        listCognitoGroups, createCognitoGroup, deleteCognitoGroup,
        adminAddUserToGroup, adminRemoveUserFromGroup, listUsersInGroup, adminListGroupsForUser,
        cognitoSignUp, cognitoInitiateAuth, cognitoGetUser,
        listIdentityPools, createIdentityPool, deleteIdentityPool, describeIdentityPool,
        setIdentityPoolRoles, getIdentityPoolRoles, cognitoGetId, cognitoGetCredentials,
        type CognitoUserPool, type CognitoUser, type CognitoUserPoolClient,
        type CognitoUserPoolClientDetail, type CognitoGroup, type CognitoUserDetail,
        type CognitoIdentityPool, type CognitoResourceServer, type CognitoIdentityProvider,
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
    let newGroupPrecedence = $state<number | null>(null);
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

    // General
    let settingDeletionProtection = $state(false);
    let savingDeletionProtection = $state(false);
    let savedDeletionProtection = $state(false);

    // MFA
    let settingMfaConfig = $state<'OFF' | 'OPTIONAL' | 'ON'>('OFF');
    let settingMfaTotp = $state(false);
    let settingMfaSms = $state(false);
    let savingMfa = $state(false);
    let savedMfa = $state(false);

    // Password Policy
    let settingPwMinLength = $state(8);
    let settingPwUppercase = $state(true);
    let settingPwLowercase = $state(true);
    let settingPwNumbers = $state(true);
    let settingPwSymbols = $state(false);
    let settingPwTempExpiry = $state(7);
    let savingPassword = $state(false);
    let savedPassword = $state(false);

    // Lambda Triggers
    const TRIGGER_KEYS = [
        'PreSignUp', 'PostConfirmation', 'PreAuthentication', 'PostAuthentication',
        'PreTokenGeneration', 'CustomMessage', 'DefineAuthChallenge',
        'CreateAuthChallenge', 'VerifyAuthChallengeResponse', 'UserMigration',
    ] as const;
    type TriggerKey = typeof TRIGGER_KEYS[number];
    let settingTriggers = $state<Record<TriggerKey, string>>({
        PreSignUp: '', PostConfirmation: '', PreAuthentication: '', PostAuthentication: '',
        PreTokenGeneration: '', CustomMessage: '', DefineAuthChallenge: '',
        CreateAuthChallenge: '', VerifyAuthChallengeResponse: '', UserMigration: '',
    });
    let savingTriggers = $state(false);
    let savedTriggers = $state(false);

    // Domain
    let currentDomain = $state('');
    let newDomainPrefix = $state('');
    let savingDomain = $state(false);
    let deletingDomain = $state(false);
    let domainError = $state<string | null>(null);

    // Auto-Verified Attributes
    let settingAutoVerifyEmail = $state(false);
    let settingAutoVerifyPhone = $state(false);
    let savingAutoVerify = $state(false);
    let savedAutoVerify = $state(false);

    // User Attribute Schema
    let poolSchemaAttrs = $state<{ name: string; type: string; mutable: boolean; required: boolean }[]>([]);
    let newCustomAttrName = $state('');
    let newCustomAttrType = $state('String');
    let addingCustomAttr = $state(false);
    let customAttrError = $state<string | null>(null);

    // Resource Servers
    let resourceServers = $state<CognitoResourceServer[]>([]);
    let resourceServersLoading = $state(false);
    let showCreateResourceServer = $state(false);
    let newRsIdentifier = $state('');
    let newRsName = $state('');
    let newRsScopes = $state<{ name: string; description: string }[]>([{ name: '', description: '' }]);
    let creatingResourceServer = $state(false);
    let resourceServerError = $state<string | null>(null);

    // Identity Providers (per pool)
    let poolIdps = $state<CognitoIdentityProvider[]>([]);
    let poolIdpsLoading = $state(false);
    let showCreateIdp = $state(false);
    let newIdpType = $state('OIDC');
    let newIdpName = $state('');
    let newIdpDetails = $state<{ key: string; value: string }[]>([{ key: 'client_id', value: '' }, { key: 'client_secret', value: '' }, { key: 'authorize_scopes', value: 'openid' }]);
    let newIdpMapping = $state<{ key: string; value: string }[]>([{ key: 'email', value: 'email' }]);
    let creatingIdp = $state(false);
    let idpError = $state<string | null>(null);

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
    let authRoleArn = $state('');
    let unauthRoleArn = $state('');
    let savingRoles = $state(false);
    let rolesSaved = $state(false);

    // ---- Auth Tester ----
    let authPoolId = $state('');
    let authClientId = $state('');
    let authClients = $state<CognitoUserPoolClient[]>([]);

    // Sign Up
    let signUpUsername = $state('');
    let signUpPassword = $state('');
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

    // Get AWS Credentials
    let credPoolId = $state('');
    let gettingCreds = $state(false);
    let awsCreds = $state<{ AccessKeyId: string; SecretKey: string; SessionToken: string; Expiration?: string } | null>(null);
    let credsError = $state<string | null>(null);
    let showSecret = $state(false);

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
            const uname = newUsername.trim();
            const isEmail = uname.includes('@');
            await adminCreateUser(selectedPool.id, uname, {
                tempPassword: newUserTempPassword || undefined,
                email: isEmail ? uname : undefined,
            });
            newUsername = '';
            newUserTempPassword = '';
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
                precedence: newGroupPrecedence != null ? newGroupPrecedence : undefined,
            });
            newGroupName = '';
            newGroupDescription = '';
            newGroupRoleArn = '';
            newGroupPrecedence = null;
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
            const pd = data as Record<string, unknown>;
            const up = (pd['UserPool'] ?? {}) as Record<string, unknown>;

            // MFA
            const mfa = String(up['MfaConfiguration'] ?? 'OFF') as 'OFF' | 'OPTIONAL' | 'ON';
            settingMfaConfig = mfa;
            const enabledMfas = (up['EnabledMfas'] as string[] | undefined) ?? [];
            settingMfaTotp = enabledMfas.includes('SOFTWARE_TOKEN_MFA');
            settingMfaSms = enabledMfas.includes('SMS_MFA');

            // Password Policy
            const policies = ((up['Policies'] as Record<string, unknown> | undefined)?.['PasswordPolicy'] as Record<string, unknown> | undefined) ?? {};
            settingPwMinLength = Number(policies['MinimumLength'] ?? 8);
            settingPwUppercase = Boolean(policies['RequireUppercase'] ?? true);
            settingPwLowercase = Boolean(policies['RequireLowercase'] ?? true);
            settingPwNumbers = Boolean(policies['RequireNumbers'] ?? true);
            settingPwSymbols = Boolean(policies['RequireSymbols'] ?? false);
            settingPwTempExpiry = Number(policies['TemporaryPasswordValidityDays'] ?? 7);

            // Deletion Protection
            settingDeletionProtection = String(up['DeletionProtection'] ?? 'INACTIVE') === 'ACTIVE';

            // Lambda Triggers
            const lambdaConfig = (up['LambdaConfig'] as Record<string, string> | undefined) ?? {};
            for (const key of TRIGGER_KEYS) {
                settingTriggers[key] = lambdaConfig[key] ?? '';
            }

            // Auto-Verified Attributes
            const autoVerified = (up['AutoVerifiedAttributes'] as string[] | undefined) ?? [];
            settingAutoVerifyEmail = autoVerified.includes('email');
            settingAutoVerifyPhone = autoVerified.includes('phone_number');

            // Schema Attributes
            const schema = (up['SchemaAttributes'] as { Name: string; AttributeDataType: string; Mutable?: boolean; Required?: boolean }[] | undefined) ?? [];
            poolSchemaAttrs = schema.map((s) => ({
                name: s.Name,
                type: s.AttributeDataType,
                mutable: s.Mutable ?? true,
                required: s.Required ?? false,
            }));

            // Domain
            currentDomain = String(up['Domain'] ?? '');

        } catch {
            poolDetail = null;
        } finally {
            poolDetailLoading = false;
        }

        // Load resource servers and IdPs in parallel
        if (selectedPool) {
            resourceServersLoading = true;
            poolIdpsLoading = true;
            try {
                const [rsData, idpData] = await Promise.all([
                    listResourceServers(selectedPool.id),
                    listIdentityProviders(selectedPool.id),
                ]);
                resourceServers = rsData.servers;
                poolIdps = idpData.providers;
            } catch {
                resourceServers = [];
                poolIdps = [];
            } finally {
                resourceServersLoading = false;
                poolIdpsLoading = false;
            }
        }
    }

    async function saveMfa() {
        if (!selectedPool) return;
        savingMfa = true;
        savedMfa = false;
        try {
            const enabledMfas: string[] = [];
            if (settingMfaTotp) enabledMfas.push('SOFTWARE_TOKEN_MFA');
            if (settingMfaSms) enabledMfas.push('SMS_MFA');
            await updateUserPool(selectedPool.id, {
                MfaConfiguration: settingMfaConfig,
                ...(enabledMfas.length > 0 ? { EnabledMfas: enabledMfas } : {}),
            });
            savedMfa = true;
            setTimeout(() => { savedMfa = false; }, 2500);
        } catch { /* ignore */ } finally {
            savingMfa = false;
        }
    }

    async function savePasswordPolicy() {
        if (!selectedPool) return;
        savingPassword = true;
        savedPassword = false;
        try {
            await updateUserPool(selectedPool.id, {
                Policies: {
                    PasswordPolicy: {
                        MinimumLength: settingPwMinLength,
                        RequireUppercase: settingPwUppercase,
                        RequireLowercase: settingPwLowercase,
                        RequireNumbers: settingPwNumbers,
                        RequireSymbols: settingPwSymbols,
                        TemporaryPasswordValidityDays: settingPwTempExpiry,
                    },
                },
            });
            savedPassword = true;
            setTimeout(() => { savedPassword = false; }, 2500);
        } catch { /* ignore */ } finally {
            savingPassword = false;
        }
    }

    async function saveDeletionProtection() {
        if (!selectedPool) return;
        savingDeletionProtection = true;
        savedDeletionProtection = false;
        try {
            await updateUserPool(selectedPool.id, {
                DeletionProtection: settingDeletionProtection ? 'ACTIVE' : 'INACTIVE',
            });
            savedDeletionProtection = true;
            setTimeout(() => { savedDeletionProtection = false; }, 2500);
        } catch { /* ignore */ } finally {
            savingDeletionProtection = false;
        }
    }

    async function saveTriggers() {
        if (!selectedPool) return;
        savingTriggers = true;
        savedTriggers = false;
        try {
            const lambdaConfig: Record<string, string> = {};
            for (const key of TRIGGER_KEYS) {
                if (settingTriggers[key].trim()) {
                    lambdaConfig[key] = settingTriggers[key].trim();
                }
            }
            await updateUserPool(selectedPool.id, { LambdaConfig: lambdaConfig });
            savedTriggers = true;
            setTimeout(() => { savedTriggers = false; }, 2500);
        } catch { /* ignore */ } finally {
            savingTriggers = false;
        }
    }

    async function saveAutoVerify() {
        if (!selectedPool) return;
        savingAutoVerify = true;
        savedAutoVerify = false;
        try {
            const attrs: string[] = [];
            if (settingAutoVerifyEmail) attrs.push('email');
            if (settingAutoVerifyPhone) attrs.push('phone_number');
            await updateUserPool(selectedPool.id, { AutoVerifiedAttributes: attrs });
            savedAutoVerify = true;
            setTimeout(() => { savedAutoVerify = false; }, 2500);
        } catch { /* ignore */ } finally {
            savingAutoVerify = false;
        }
    }

    async function handleCreateDomain() {
        if (!selectedPool || !newDomainPrefix.trim()) return;
        savingDomain = true;
        domainError = null;
        try {
            await createUserPoolDomain(selectedPool.id, newDomainPrefix.trim());
            currentDomain = newDomainPrefix.trim();
            newDomainPrefix = '';
        } catch (e) {
            domainError = e instanceof Error ? e.message : 'Failed to create domain';
        } finally {
            savingDomain = false;
        }
    }

    async function handleDeleteDomain() {
        if (!selectedPool || !currentDomain) return;
        deletingDomain = true;
        domainError = null;
        try {
            await deleteUserPoolDomain(selectedPool.id, currentDomain);
            currentDomain = '';
        } catch (e) {
            domainError = e instanceof Error ? e.message : 'Failed to delete domain';
        } finally {
            deletingDomain = false;
        }
    }

    async function handleAddCustomAttr() {
        if (!selectedPool || !newCustomAttrName.trim()) return;
        addingCustomAttr = true;
        customAttrError = null;
        try {
            await addCustomAttributes(selectedPool.id, [{ Name: newCustomAttrName.trim(), AttributeDataType: newCustomAttrType }]);
            poolSchemaAttrs = [...poolSchemaAttrs, { name: `custom:${newCustomAttrName.trim()}`, type: newCustomAttrType, mutable: true, required: false }];
            newCustomAttrName = '';
            newCustomAttrType = 'String';
        } catch (e) {
            customAttrError = e instanceof Error ? e.message : 'Failed to add attribute';
        } finally {
            addingCustomAttr = false;
        }
    }

    async function handleCreateResourceServer() {
        if (!selectedPool || !newRsIdentifier.trim() || !newRsName.trim()) return;
        creatingResourceServer = true;
        resourceServerError = null;
        try {
            await createResourceServer(selectedPool.id, newRsIdentifier.trim(), newRsName.trim(),
                newRsScopes.filter((s) => s.name.trim()));
            showCreateResourceServer = false;
            newRsIdentifier = '';
            newRsName = '';
            newRsScopes = [{ name: '', description: '' }];
            const data = await listResourceServers(selectedPool.id);
            resourceServers = data.servers;
        } catch (e) {
            resourceServerError = e instanceof Error ? e.message : 'Failed to create resource server';
        } finally {
            creatingResourceServer = false;
        }
    }

    async function handleDeleteResourceServer(identifier: string) {
        if (!selectedPool) return;
        try {
            await deleteResourceServer(selectedPool.id, identifier);
            resourceServers = resourceServers.filter((s) => s.identifier !== identifier);
        } catch { /* ignore */ }
    }

    async function handleCreateIdp() {
        if (!selectedPool || !newIdpName.trim() || !newIdpType) return;
        creatingIdp = true;
        idpError = null;
        try {
            const details: Record<string, string> = {};
            for (const e of newIdpDetails) {
                if (e.key.trim()) details[e.key.trim()] = e.value;
            }
            const mapping: Record<string, string> = {};
            for (const e of newIdpMapping) {
                if (e.key.trim()) mapping[e.key.trim()] = e.value;
            }
            await createIdentityProvider(selectedPool.id, newIdpName.trim(), newIdpType, details, mapping);
            showCreateIdp = false;
            newIdpName = '';
            newIdpType = 'OIDC';
            newIdpDetails = [{ key: 'client_id', value: '' }, { key: 'client_secret', value: '' }, { key: 'authorize_scopes', value: 'openid' }];
            newIdpMapping = [{ key: 'email', value: 'email' }];
            const data = await listIdentityProviders(selectedPool.id);
            poolIdps = data.providers;
        } catch (e) {
            idpError = e instanceof Error ? e.message : 'Failed to create IdP';
        } finally {
            creatingIdp = false;
        }
    }

    async function handleDeleteIdp(providerName: string) {
        if (!selectedPool) return;
        try {
            await deleteIdentityProvider(selectedPool.id, providerName);
            poolIdps = poolIdps.filter((p) => p.providerName !== providerName);
        } catch { /* ignore */ }
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
        authRoleArn = '';
        unauthRoleArn = '';
        rolesSaved = false;
        try {
            identityPoolDetail = await describeIdentityPool(pool.id);
            // Load existing roles
            try {
                const rolesData = await getIdentityPoolRoles(pool.id) as Record<string, unknown>;
                const roles = (rolesData['Roles'] ?? {}) as Record<string, string>;
                authRoleArn = roles['authenticated'] ?? '';
                unauthRoleArn = roles['unauthenticated'] ?? '';
            } catch { /* roles not available */ }
        } catch {
            identityPoolDetail = null;
        } finally {
            identityPoolDetailLoading = false;
        }
    }

    async function saveRoles() {
        if (!selectedIdentityPool) return;
        savingRoles = true;
        rolesSaved = false;
        try {
            const roles: Record<string, string> = {};
            if (authRoleArn.trim()) roles['authenticated'] = authRoleArn.trim();
            if (unauthRoleArn.trim()) roles['unauthenticated'] = unauthRoleArn.trim();
            await setIdentityPoolRoles(selectedIdentityPool.id, roles);
            rolesSaved = true;
            setTimeout(() => { rolesSaved = false; }, 2500);
        } catch { /* ignore */ } finally {
            savingRoles = false;
        }
    }

    async function handleGetCredentials() {
        if (!credPoolId || !authTokens?.idToken) return;
        gettingCreds = true;
        credsError = null;
        awsCreds = null;
        try {
            // Find the issuer from the ID token to build the login key
            const decoded = decodeJwt(authTokens.idToken);
            const iss = decoded?.payload ? String((decoded.payload as Record<string, unknown>)['iss'] ?? '') : '';
            const loginKey = iss.replace('https://', '');
            const logins = loginKey ? { [loginKey]: authTokens.idToken } : undefined;

            const idData = await cognitoGetId(credPoolId, logins) as Record<string, unknown>;
            const identityId = String(idData['IdentityId'] ?? '');
            if (!identityId) throw new Error('No IdentityId returned');

            const credData = await cognitoGetCredentials(identityId, logins) as Record<string, unknown>;
            const creds = (credData['Credentials'] ?? {}) as Record<string, unknown>;
            awsCreds = {
                AccessKeyId: String(creds['AccessKeyId'] ?? ''),
                SecretKey: String(creds['SecretKey'] ?? ''),
                SessionToken: String(creds['SessionToken'] ?? ''),
                Expiration: creds['Expiration'] ? String(creds['Expiration']) : undefined,
            };
        } catch (e) {
            credsError = e instanceof Error ? e.message : 'Failed to get credentials';
        } finally {
            gettingCreds = false;
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
            const email = signUpUsername.includes('@') ? signUpUsername : undefined;
            signUpResult = await cognitoSignUp(authClientId, signUpUsername, signUpPassword, email);
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
                                                        placeholder="Username or email"
                                                    />
                                                    <input
                                                        type="password"
                                                        bind:value={newUserTempPassword}
                                                        class="bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200"
                                                        placeholder="Temporary password (optional)"
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
                                                        onclick={() => { showCreateUser = false; createUserError = null; newUsername = ''; newUserTempPassword = ''; }}
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
                                                            {#each userGroups.slice().sort((a, b) => (a.precedence ?? 999) - (b.precedence ?? 999)) as g}
                                                                <div class="flex items-center gap-2 px-3 py-2 border-b border-zinc-700/50 last:border-0">
                                                                    <span class="text-xs font-mono text-zinc-200 shrink-0">{g.name}</span>
                                                                    {#if g.precedence != null}
                                                                        <span class="text-xs px-1.5 py-0.5 rounded bg-zinc-700 text-zinc-400 shrink-0">P:{g.precedence}</span>
                                                                    {/if}
                                                                    {#if g.roleArn}
                                                                        <span class="text-xs font-mono text-blue-400 truncate flex-1" title={g.roleArn}>{g.roleArn.split('/').pop() ?? g.roleArn}</span>
                                                                    {/if}
                                                                    <button onclick={() => handleRemoveFromGroup(g.name)} class="text-xs text-red-400 hover:text-red-300 transition-colors shrink-0 ml-auto">Remove</button>
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
                                                    <input
                                                        type="number"
                                                        bind:value={newGroupPrecedence}
                                                        class="bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200"
                                                        placeholder="Precedence (lower = higher priority, optional)"
                                                        min="0"
                                                    />
                                                </div>
                                                <div class="flex gap-2">
                                                    <button onclick={handleCreateGroup} disabled={creatingGroup || !newGroupName.trim()} class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-xs font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">{creatingGroup ? 'Creating...' : 'Create'}</button>
                                                    <button onclick={() => { showCreateGroup = false; createGroupError = null; newGroupName = ''; newGroupDescription = ''; newGroupRoleArn = ''; newGroupPrecedence = null; }} class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-orange-500/50">Cancel</button>
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
                                                        <th class="px-4 py-2 text-xs">Role ARN</th>
                                                        <th class="px-4 py-2 text-xs">Precedence</th>
                                                        <th class="px-4 py-2 text-xs"></th>
                                                    </tr>
                                                </thead>
                                                <tbody>
                                                    {#each groups as group}
                                                        <tr class="border-b border-zinc-800/50 cursor-pointer {selectedGroup?.name === group.name ? 'bg-zinc-800' : 'hover:bg-zinc-800/30'}" onclick={() => selectGroup(group)}>
                                                            <td class="px-4 py-2.5 font-mono text-orange-400 text-xs">{group.name}</td>
                                                            <td class="px-4 py-2.5 text-xs max-w-[140px]">
                                                                {#if group.roleArn}
                                                                    <span class="font-mono text-blue-400 truncate block" title={group.roleArn}>{group.roleArn.split('/').pop() ?? group.roleArn}</span>
                                                                {:else}
                                                                    <span class="text-zinc-600">—</span>
                                                                {/if}
                                                            </td>
                                                            <td class="px-4 py-2.5 text-xs">
                                                                {#if group.precedence != null}
                                                                    <span class="px-1.5 py-0.5 rounded bg-zinc-700 text-zinc-300">{group.precedence}</span>
                                                                {:else}
                                                                    <span class="text-zinc-600">—</span>
                                                                {/if}
                                                            </td>
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
                                            <div class="space-y-2 mb-3">
                                                {#if selectedGroup.precedence != null}
                                                    <div class="flex items-center gap-2">
                                                        <span class="text-xs text-zinc-500">Precedence:</span>
                                                        <span class="text-xs px-1.5 py-0.5 rounded bg-zinc-700 text-zinc-300">{selectedGroup.precedence}</span>
                                                    </div>
                                                {/if}
                                                {#if selectedGroup.roleArn}
                                                    <div>
                                                        <span class="text-xs text-zinc-500 block mb-0.5">Role ARN</span>
                                                        <span class="text-xs font-mono text-blue-400 break-all">{selectedGroup.roleArn}</span>
                                                    </div>
                                                {/if}
                                            </div>
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
                                <div class="p-4 overflow-auto">
                                    {#if poolDetailLoading}
                                        <div class="text-zinc-500 text-sm">Loading settings...</div>
                                    {:else if !poolDetail}
                                        <div class="text-zinc-500 text-sm">No settings available.</div>
                                    {:else}
                                        <div class="space-y-4">

                                            <!-- 1. General Settings -->
                                            <div class="bg-zinc-800/50 rounded-lg border border-zinc-800 p-4">
                                                <h4 class="text-sm font-medium text-zinc-200 flex items-center gap-2 mb-3">
                                                    <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                                                    General Settings
                                                </h4>
                                                <div class="space-y-3">
                                                    <div class="flex items-center justify-between">
                                                        <div>
                                                            <div class="text-xs text-zinc-400">Pool Name</div>
                                                            <div class="text-sm text-zinc-200">{selectedPool?.name}</div>
                                                        </div>
                                                    </div>
                                                    <div class="flex items-center justify-between">
                                                        <div>
                                                            <div class="text-xs text-zinc-400">Pool ID</div>
                                                            <div class="text-xs font-mono text-zinc-300 bg-zinc-800 rounded px-2 py-1 mt-0.5">{selectedPool?.id}</div>
                                                        </div>
                                                    </div>
                                                    <div class="flex items-center justify-between pt-1">
                                                        <div>
                                                            <div class="text-xs text-zinc-200">Deletion Protection</div>
                                                            <div class="text-xs text-zinc-500 mt-0.5">Prevent this pool from being deleted</div>
                                                        </div>
                                                        <div class="flex items-center gap-3">
                                                            {#if savedDeletionProtection}
                                                                <span class="text-xs text-green-400">Saved</span>
                                                            {/if}
                                                            <button
                                                                onclick={() => { settingDeletionProtection = !settingDeletionProtection; }}
                                                                aria-label="Toggle deletion protection"
                                                                class="relative w-10 h-5 rounded-full transition-colors {settingDeletionProtection ? 'bg-orange-600' : 'bg-zinc-700'}"
                                                            >
                                                                <span class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {settingDeletionProtection ? 'translate-x-5' : ''}"></span>
                                                            </button>
                                                            <button
                                                                onclick={saveDeletionProtection}
                                                                disabled={savingDeletionProtection}
                                                                class="px-3 py-1 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-xs font-medium transition-all"
                                                            >
                                                                {savingDeletionProtection ? 'Saving...' : 'Save'}
                                                            </button>
                                                        </div>
                                                    </div>
                                                </div>
                                            </div>

                                            <!-- 2. Sign-in Experience (MFA) -->
                                            <div class="bg-zinc-800/50 rounded-lg border border-zinc-800 p-4">
                                                <div class="flex items-center justify-between mb-3">
                                                    <h4 class="text-sm font-medium text-zinc-200 flex items-center gap-2">
                                                        <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                                                        Sign-in Experience
                                                    </h4>
                                                    <div class="flex items-center gap-2">
                                                        {#if savedMfa}
                                                            <span class="text-xs text-green-400">Saved</span>
                                                        {/if}
                                                        <button onclick={saveMfa} disabled={savingMfa}
                                                            class="px-3 py-1 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-xs font-medium transition-all">
                                                            {savingMfa ? 'Saving...' : 'Save'}
                                                        </button>
                                                    </div>
                                                </div>
                                                <div class="space-y-4">
                                                    <div>
                                                        <div class="text-xs text-zinc-400 mb-2">MFA Configuration</div>
                                                        <div class="flex gap-4">
                                                            {#each (['OFF', 'OPTIONAL', 'ON'] as const) as option}
                                                                <label class="flex items-center gap-2 cursor-pointer">
                                                                    <input type="radio" name="mfa-config" value={option} bind:group={settingMfaConfig}
                                                                        class="text-orange-500 focus:ring-orange-500/50 accent-orange-500" />
                                                                    <span class="text-sm {settingMfaConfig === option ? 'text-zinc-200' : 'text-zinc-500'}">{option}</span>
                                                                </label>
                                                            {/each}
                                                        </div>
                                                    </div>
                                                    {#if settingMfaConfig !== 'OFF'}
                                                        <div>
                                                            <div class="text-xs text-zinc-400 mb-2">MFA Methods</div>
                                                            <div class="space-y-2">
                                                                <label class="flex items-center gap-2 cursor-pointer">
                                                                    <input type="checkbox" bind:checked={settingMfaTotp} class="rounded accent-orange-500" />
                                                                    <span class="text-sm text-zinc-300">TOTP (Authenticator app)</span>
                                                                </label>
                                                                <label class="flex items-center gap-2 cursor-pointer">
                                                                    <input type="checkbox" bind:checked={settingMfaSms} class="rounded accent-orange-500" />
                                                                    <span class="text-sm text-zinc-300">SMS (stub)</span>
                                                                </label>
                                                            </div>
                                                        </div>
                                                    {/if}
                                                </div>
                                            </div>

                                            <!-- 3. Password Policy -->
                                            <div class="bg-zinc-800/50 rounded-lg border border-zinc-800 p-4">
                                                <div class="flex items-center justify-between mb-3">
                                                    <h4 class="text-sm font-medium text-zinc-200 flex items-center gap-2">
                                                        <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                                                        Password Policy
                                                    </h4>
                                                    <div class="flex items-center gap-2">
                                                        {#if savedPassword}
                                                            <span class="text-xs text-green-400">Saved</span>
                                                        {/if}
                                                        <button onclick={savePasswordPolicy} disabled={savingPassword}
                                                            class="px-3 py-1 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-xs font-medium transition-all">
                                                            {savingPassword ? 'Saving...' : 'Save'}
                                                        </button>
                                                    </div>
                                                </div>
                                                <div class="space-y-3">
                                                    <div class="flex items-center justify-between">
                                                        <label class="text-xs text-zinc-300">Minimum length</label>
                                                        <input
                                                            type="number" min="6" max="99"
                                                            bind:value={settingPwMinLength}
                                                            class="w-20 bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-xs text-zinc-200 focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30"
                                                        />
                                                    </div>
                                                    <div class="flex items-center justify-between">
                                                        <label class="text-xs text-zinc-300">Temporary password expiry (days)</label>
                                                        <input
                                                            type="number" min="1" max="365"
                                                            bind:value={settingPwTempExpiry}
                                                            class="w-20 bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-xs text-zinc-200 focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30"
                                                        />
                                                    </div>
                                                    {#each [
                                                        { label: 'Require uppercase', key: 'settingPwUppercase', get: () => settingPwUppercase, set: (v: boolean) => { settingPwUppercase = v; } },
                                                        { label: 'Require lowercase', key: 'settingPwLowercase', get: () => settingPwLowercase, set: (v: boolean) => { settingPwLowercase = v; } },
                                                        { label: 'Require numbers', key: 'settingPwNumbers', get: () => settingPwNumbers, set: (v: boolean) => { settingPwNumbers = v; } },
                                                        { label: 'Require symbols', key: 'settingPwSymbols', get: () => settingPwSymbols, set: (v: boolean) => { settingPwSymbols = v; } },
                                                    ] as item}
                                                        <div class="flex items-center justify-between">
                                                            <span class="text-xs text-zinc-300">{item.label}</span>
                                                            <button
                                                                onclick={() => item.set(!item.get())}
                                                                aria-label={item.label}
                                                                class="relative w-10 h-5 rounded-full transition-colors {item.get() ? 'bg-orange-600' : 'bg-zinc-700'}"
                                                            >
                                                                <span class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {item.get() ? 'translate-x-5' : ''}"></span>
                                                            </button>
                                                        </div>
                                                    {/each}
                                                </div>
                                            </div>

                                            <!-- 4. Lambda Triggers -->
                                            <div class="bg-zinc-800/50 rounded-lg border border-zinc-800 p-4">
                                                <div class="flex items-center justify-between mb-3">
                                                    <h4 class="text-sm font-medium text-zinc-200 flex items-center gap-2">
                                                        <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                                                        Lambda Triggers
                                                    </h4>
                                                    <div class="flex items-center gap-2">
                                                        {#if savedTriggers}
                                                            <span class="text-xs text-green-400">Saved</span>
                                                        {/if}
                                                        <button onclick={saveTriggers} disabled={savingTriggers}
                                                            class="px-3 py-1 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-xs font-medium transition-all">
                                                            {savingTriggers ? 'Saving...' : 'Save Triggers'}
                                                        </button>
                                                    </div>
                                                </div>
                                                <div class="space-y-2">
                                                    {#each TRIGGER_KEYS as key}
                                                        <div class="flex items-center gap-2">
                                                            <span class="text-xs text-zinc-400 w-52 shrink-0">{key}</span>
                                                            <input
                                                                bind:value={settingTriggers[key]}
                                                                class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-xs font-mono text-zinc-200 focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30"
                                                                placeholder="arn:aws:lambda:..."
                                                            />
                                                            {#if settingTriggers[key]}
                                                                <button
                                                                    onclick={() => { settingTriggers[key] = ''; }}
                                                                    class="text-red-400 hover:text-red-300 text-xs px-1 transition-colors"
                                                                >x</button>
                                                            {/if}
                                                        </div>
                                                    {/each}
                                                </div>
                                            </div>

                                            <!-- 5. Domain -->
                                            <div class="bg-zinc-800/50 rounded-lg border border-zinc-800 p-4">
                                                <h4 class="text-sm font-medium text-zinc-200 flex items-center gap-2 mb-3">
                                                    <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                                                    Domain
                                                </h4>
                                                {#if domainError}
                                                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs mb-3">{domainError}</div>
                                                {/if}
                                                {#if currentDomain}
                                                    <div class="flex items-center justify-between bg-zinc-800 rounded px-3 py-2 mb-2">
                                                        <div>
                                                            <div class="text-xs text-zinc-400 mb-0.5">Current domain</div>
                                                            <div class="text-xs font-mono text-orange-400">{currentDomain}</div>
                                                            <div class="text-xs text-zinc-500 mt-1 font-mono">
                                                                http://localhost:4566/cognito/{selectedPool?.id}/oauth2/authorize
                                                            </div>
                                                        </div>
                                                        <button
                                                            onclick={handleDeleteDomain}
                                                            disabled={deletingDomain}
                                                            class="px-2 py-1 bg-red-900/30 text-red-400 hover:bg-red-900/50 rounded text-xs transition-all disabled:opacity-50"
                                                        >
                                                            {deletingDomain ? '...' : 'Delete'}
                                                        </button>
                                                    </div>
                                                {:else}
                                                    <div class="flex gap-2">
                                                        <input
                                                            bind:value={newDomainPrefix}
                                                            class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-xs text-zinc-200 focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30"
                                                            placeholder="Domain prefix"
                                                        />
                                                        <button
                                                            onclick={handleCreateDomain}
                                                            disabled={savingDomain || !newDomainPrefix.trim()}
                                                            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-xs font-medium transition-all"
                                                        >
                                                            {savingDomain ? 'Creating...' : 'Create'}
                                                        </button>
                                                    </div>
                                                {/if}
                                            </div>

                                            <!-- 6. Auto-Verified Attributes -->
                                            <div class="bg-zinc-800/50 rounded-lg border border-zinc-800 p-4">
                                                <div class="flex items-center justify-between mb-3">
                                                    <h4 class="text-sm font-medium text-zinc-200 flex items-center gap-2">
                                                        <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                                                        Auto-Verified Attributes
                                                    </h4>
                                                    <div class="flex items-center gap-2">
                                                        {#if savedAutoVerify}
                                                            <span class="text-xs text-green-400">Saved</span>
                                                        {/if}
                                                        <button onclick={saveAutoVerify} disabled={savingAutoVerify}
                                                            class="px-3 py-1 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-xs font-medium transition-all">
                                                            {savingAutoVerify ? 'Saving...' : 'Save'}
                                                        </button>
                                                    </div>
                                                </div>
                                                <div class="space-y-2">
                                                    <label class="flex items-center gap-2 cursor-pointer">
                                                        <input type="checkbox" bind:checked={settingAutoVerifyEmail} class="rounded accent-orange-500" />
                                                        <span class="text-sm text-zinc-300">email</span>
                                                    </label>
                                                    <label class="flex items-center gap-2 cursor-pointer">
                                                        <input type="checkbox" bind:checked={settingAutoVerifyPhone} class="rounded accent-orange-500" />
                                                        <span class="text-sm text-zinc-300">phone_number</span>
                                                    </label>
                                                </div>
                                            </div>

                                            <!-- 7. User Attribute Schema -->
                                            <div class="bg-zinc-800/50 rounded-lg border border-zinc-800 p-4">
                                                <h4 class="text-sm font-medium text-zinc-200 flex items-center gap-2 mb-3">
                                                    <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                                                    User Attribute Schema
                                                </h4>
                                                {#if poolSchemaAttrs.length > 0}
                                                    <div class="bg-zinc-900 rounded overflow-hidden mb-3">
                                                        <table class="w-full text-xs">
                                                            <thead>
                                                                <tr class="border-b border-zinc-800 text-zinc-500">
                                                                    <th class="px-3 py-2 text-left">Name</th>
                                                                    <th class="px-3 py-2 text-left">Type</th>
                                                                    <th class="px-3 py-2 text-left">Mutable</th>
                                                                    <th class="px-3 py-2 text-left">Required</th>
                                                                </tr>
                                                            </thead>
                                                            <tbody>
                                                                {#each poolSchemaAttrs as attr}
                                                                    <tr class="border-b border-zinc-800/50 last:border-0">
                                                                        <td class="px-3 py-1.5 font-mono text-zinc-300">{attr.name}</td>
                                                                        <td class="px-3 py-1.5 text-zinc-400">{attr.type}</td>
                                                                        <td class="px-3 py-1.5 {attr.mutable ? 'text-green-400' : 'text-zinc-500'}">{attr.mutable ? 'Yes' : 'No'}</td>
                                                                        <td class="px-3 py-1.5 {attr.required ? 'text-orange-400' : 'text-zinc-500'}">{attr.required ? 'Yes' : 'No'}</td>
                                                                    </tr>
                                                                {/each}
                                                            </tbody>
                                                        </table>
                                                    </div>
                                                {:else}
                                                    <div class="text-xs text-zinc-500 mb-3">No attributes defined.</div>
                                                {/if}
                                                {#if customAttrError}
                                                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs mb-2">{customAttrError}</div>
                                                {/if}
                                                <div class="flex gap-2 items-center">
                                                    <span class="text-xs text-zinc-400 shrink-0">Add custom:</span>
                                                    <input
                                                        bind:value={newCustomAttrName}
                                                        class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-xs text-zinc-200 focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30"
                                                        placeholder="attribute name"
                                                    />
                                                    <select
                                                        bind:value={newCustomAttrType}
                                                        class="bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-xs text-zinc-200 focus:outline-none focus:border-orange-500 [&>option]:bg-zinc-800"
                                                    >
                                                        <option value="String">String</option>
                                                        <option value="Number">Number</option>
                                                        <option value="DateTime">DateTime</option>
                                                        <option value="Boolean">Boolean</option>
                                                    </select>
                                                    <button
                                                        onclick={handleAddCustomAttr}
                                                        disabled={addingCustomAttr || !newCustomAttrName.trim()}
                                                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-xs font-medium transition-all"
                                                    >
                                                        {addingCustomAttr ? '...' : 'Add'}
                                                    </button>
                                                </div>
                                            </div>

                                            <!-- 8. Resource Servers -->
                                            <div class="bg-zinc-800/50 rounded-lg border border-zinc-800 p-4">
                                                <div class="flex items-center justify-between mb-3">
                                                    <h4 class="text-sm font-medium text-zinc-200 flex items-center gap-2">
                                                        <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                                                        Resource Servers
                                                    </h4>
                                                    <button
                                                        onclick={() => { showCreateResourceServer = !showCreateResourceServer; resourceServerError = null; }}
                                                        class="px-3 py-1 bg-orange-600 hover:bg-orange-500 rounded text-xs font-medium transition-all"
                                                    >
                                                        {showCreateResourceServer ? 'Cancel' : 'Create'}
                                                    </button>
                                                </div>

                                                {#if showCreateResourceServer}
                                                    <div class="bg-zinc-900 rounded p-3 mb-3 space-y-2">
                                                        {#if resourceServerError}
                                                            <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs">{resourceServerError}</div>
                                                        {/if}
                                                        <input bind:value={newRsIdentifier} class="w-full bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-xs text-zinc-200 focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 font-mono" placeholder="Identifier (e.g. https://api.example.com)" />
                                                        <input bind:value={newRsName} class="w-full bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-xs text-zinc-200 focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30" placeholder="Name" />
                                                        <div class="space-y-1">
                                                            <span class="text-xs text-zinc-400">Scopes</span>
                                                            {#each newRsScopes as scope, i}
                                                                <div class="flex gap-2">
                                                                    <input bind:value={newRsScopes[i].name} class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-xs text-zinc-200 focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30" placeholder="Scope name" />
                                                                    <input bind:value={newRsScopes[i].description} class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-xs text-zinc-200 focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30" placeholder="Description" />
                                                                    <button onclick={() => { newRsScopes = newRsScopes.filter((_, j) => j !== i); }} class="text-red-400 hover:text-red-300 text-xs px-1">x</button>
                                                                </div>
                                                            {/each}
                                                            <button onclick={() => { newRsScopes = [...newRsScopes, { name: '', description: '' }]; }} class="text-xs text-orange-400 hover:text-orange-300">+ Add Scope</button>
                                                        </div>
                                                        <button
                                                            onclick={handleCreateResourceServer}
                                                            disabled={creatingResourceServer || !newRsIdentifier.trim() || !newRsName.trim()}
                                                            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-xs font-medium transition-all"
                                                        >
                                                            {creatingResourceServer ? 'Creating...' : 'Create Resource Server'}
                                                        </button>
                                                    </div>
                                                {/if}

                                                {#if resourceServersLoading}
                                                    <div class="text-xs text-zinc-500">Loading...</div>
                                                {:else if resourceServers.length === 0}
                                                    <div class="text-xs text-zinc-500">No resource servers configured.</div>
                                                {:else}
                                                    <div class="space-y-2">
                                                        {#each resourceServers as rs}
                                                            <div class="bg-zinc-900 rounded p-3">
                                                                <div class="flex items-start justify-between gap-2">
                                                                    <div class="min-w-0">
                                                                        <div class="text-xs font-semibold text-zinc-200">{rs.name}</div>
                                                                        <div class="text-xs font-mono text-zinc-400 truncate">{rs.identifier}</div>
                                                                        {#if rs.scopes.length > 0}
                                                                            <div class="flex flex-wrap gap-1 mt-1">
                                                                                {#each rs.scopes as scope}
                                                                                    <span class="px-1.5 py-0.5 bg-zinc-700 rounded text-xs font-mono">{scope.name}</span>
                                                                                {/each}
                                                                            </div>
                                                                        {/if}
                                                                    </div>
                                                                    <button onclick={() => handleDeleteResourceServer(rs.identifier)} class="text-red-400 hover:text-red-300 text-xs shrink-0 transition-colors">Delete</button>
                                                                </div>
                                                            </div>
                                                        {/each}
                                                    </div>
                                                {/if}
                                            </div>

                                            <!-- 9. Identity Providers -->
                                            <div class="bg-zinc-800/50 rounded-lg border border-zinc-800 p-4">
                                                <div class="flex items-center justify-between mb-3">
                                                    <h4 class="text-sm font-medium text-zinc-200 flex items-center gap-2">
                                                        <span class="w-2 h-2 rounded-full bg-orange-500"></span>
                                                        Identity Providers
                                                    </h4>
                                                    <button
                                                        onclick={() => { showCreateIdp = !showCreateIdp; idpError = null; }}
                                                        class="px-3 py-1 bg-orange-600 hover:bg-orange-500 rounded text-xs font-medium transition-all"
                                                    >
                                                        {showCreateIdp ? 'Cancel' : 'Add Provider'}
                                                    </button>
                                                </div>

                                                {#if showCreateIdp}
                                                    <div class="bg-zinc-900 rounded p-3 mb-3 space-y-3">
                                                        {#if idpError}
                                                            <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs">{idpError}</div>
                                                        {/if}
                                                        <div class="grid grid-cols-2 gap-2">
                                                            <div>
                                                                <label class="text-xs text-zinc-400 block mb-1">Type</label>
                                                                <select bind:value={newIdpType} class="w-full bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-xs text-zinc-200 focus:outline-none focus:border-orange-500 [&>option]:bg-zinc-800">
                                                                    <option value="OIDC">OIDC</option>
                                                                    <option value="SAML">SAML</option>
                                                                    <option value="Google">Google</option>
                                                                    <option value="Facebook">Facebook</option>
                                                                    <option value="SignInWithApple">Apple</option>
                                                                    <option value="LoginWithAmazon">Amazon</option>
                                                                </select>
                                                            </div>
                                                            <div>
                                                                <label class="text-xs text-zinc-400 block mb-1">Provider Name</label>
                                                                <input bind:value={newIdpName} class="w-full bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-xs text-zinc-200 focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30" placeholder="e.g. MyOIDCProvider" />
                                                            </div>
                                                        </div>

                                                        <div>
                                                            <div class="text-xs text-zinc-400 mb-1">Provider Details</div>
                                                            <div class="space-y-1.5">
                                                                {#each newIdpDetails as entry, i}
                                                                    <div class="flex gap-2">
                                                                        <input bind:value={newIdpDetails[i].key} class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-xs text-zinc-200 focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30" placeholder="Key" />
                                                                        <input bind:value={newIdpDetails[i].value} class="flex-2 bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-xs font-mono text-zinc-200 focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 w-48" placeholder="Value" />
                                                                        <button onclick={() => { newIdpDetails = newIdpDetails.filter((_, j) => j !== i); }} class="text-red-400 hover:text-red-300 text-xs px-1">x</button>
                                                                    </div>
                                                                {/each}
                                                                <button onclick={() => { newIdpDetails = [...newIdpDetails, { key: '', value: '' }]; }} class="text-xs text-orange-400 hover:text-orange-300">+ Add</button>
                                                            </div>
                                                        </div>

                                                        <div>
                                                            <div class="text-xs text-zinc-400 mb-1">Attribute Mapping</div>
                                                            <div class="space-y-1.5">
                                                                {#each newIdpMapping as entry, i}
                                                                    <div class="flex gap-2">
                                                                        <input bind:value={newIdpMapping[i].key} class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-xs text-zinc-200 focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30" placeholder="User pool attr" />
                                                                        <input bind:value={newIdpMapping[i].value} class="flex-2 bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-xs font-mono text-zinc-200 focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 w-48" placeholder="IdP attribute" />
                                                                        <button onclick={() => { newIdpMapping = newIdpMapping.filter((_, j) => j !== i); }} class="text-red-400 hover:text-red-300 text-xs px-1">x</button>
                                                                    </div>
                                                                {/each}
                                                                <button onclick={() => { newIdpMapping = [...newIdpMapping, { key: '', value: '' }]; }} class="text-xs text-orange-400 hover:text-orange-300">+ Add</button>
                                                            </div>
                                                        </div>

                                                        <button
                                                            onclick={handleCreateIdp}
                                                            disabled={creatingIdp || !newIdpName.trim()}
                                                            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-xs font-medium transition-all"
                                                        >
                                                            {creatingIdp ? 'Creating...' : 'Create Identity Provider'}
                                                        </button>
                                                    </div>
                                                {/if}

                                                {#if poolIdpsLoading}
                                                    <div class="text-xs text-zinc-500">Loading...</div>
                                                {:else if poolIdps.length === 0}
                                                    <div class="text-xs text-zinc-500">No identity providers configured.</div>
                                                {:else}
                                                    <div class="space-y-2">
                                                        {#each poolIdps as idp}
                                                            <div class="bg-zinc-900 rounded p-3 flex items-center justify-between gap-2">
                                                                <div>
                                                                    <span class="text-xs font-semibold text-zinc-200">{idp.providerName}</span>
                                                                    <span class="ml-2 px-1.5 py-0.5 bg-zinc-700 rounded text-xs text-zinc-400">{idp.providerType}</span>
                                                                    {#if idp.creationDate}
                                                                        <div class="text-xs text-zinc-500 mt-0.5">{formatDate(idp.creationDate)}</div>
                                                                    {/if}
                                                                </div>
                                                                <button onclick={() => handleDeleteIdp(idp.providerName)} class="text-red-400 hover:text-red-300 text-xs shrink-0 transition-colors">Delete</button>
                                                            </div>
                                                        {/each}
                                                    </div>
                                                {/if}
                                            </div>

                                        </div>
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
                                            <h4 class="text-xs font-medium text-zinc-400 uppercase tracking-wider mb-2">Current Roles</h4>
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

                                    <!-- Role Mapping Editor -->
                                    <div class="mt-4">
                                        <h4 class="text-xs font-medium text-zinc-400 uppercase tracking-wider mb-2 flex items-center gap-2">
                                            <span class="w-2 h-2 rounded-full bg-blue-500"></span>
                                            Role Mapping
                                        </h4>
                                        <div class="bg-zinc-800/50 rounded p-3 space-y-3">
                                            <div>
                                                <label class="text-xs text-zinc-500 block mb-1">Authenticated Role ARN</label>
                                                <input
                                                    type="text"
                                                    bind:value={authRoleArn}
                                                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-xs font-mono focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500/30 text-zinc-200"
                                                    placeholder="arn:aws:iam::000000000000:role/AuthenticatedRole"
                                                />
                                            </div>
                                            <div>
                                                <label class="text-xs text-zinc-500 block mb-1">Unauthenticated Role ARN</label>
                                                <input
                                                    type="text"
                                                    bind:value={unauthRoleArn}
                                                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-xs font-mono focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500/30 text-zinc-200"
                                                    placeholder="arn:aws:iam::000000000000:role/UnauthenticatedRole"
                                                />
                                            </div>
                                            <div class="flex items-center gap-2">
                                                <button
                                                    onclick={saveRoles}
                                                    disabled={savingRoles}
                                                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-xs font-medium transition-all active:scale-[0.98]"
                                                >
                                                    {savingRoles ? 'Saving...' : 'Save Roles'}
                                                </button>
                                                {#if rolesSaved}
                                                    <span class="text-xs text-green-400">Saved</span>
                                                {/if}
                                            </div>
                                        </div>
                                    </div>
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
                        <input bind:value={signUpUsername} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200" placeholder="Username or email" />
                        <input type="password" bind:value={signUpPassword} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200" placeholder="Password" />
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
                        <input bind:value={signInUsername} class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200" placeholder="Username or email" />
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

                <!-- Get AWS Credentials -->
                {#if authTokens?.idToken}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4 shadow-lg shadow-black/20">
                        <h3 class="font-semibold mb-3 flex items-center gap-2">
                            <span class="w-2 h-2 rounded-full bg-blue-500"></span>
                            Get AWS Credentials
                        </h3>
                        <div class="space-y-3">
                            <div>
                                <label class="block text-xs text-zinc-400 mb-1">Identity Pool</label>
                                <select
                                    bind:value={credPoolId}
                                    class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500 focus:ring-1 focus:ring-orange-500/30 text-zinc-200 [&>option]:bg-zinc-800 [&>option]:text-zinc-200"
                                >
                                    <option value="">Select identity pool...</option>
                                    {#each identityPools as pool}
                                        <option value={pool.id}>{pool.name} ({pool.id})</option>
                                    {/each}
                                </select>
                            </div>
                            <button
                                onclick={handleGetCredentials}
                                disabled={!credPoolId || gettingCreds}
                                class="px-3 py-1.5 bg-blue-600 hover:bg-blue-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-all active:scale-[0.98] focus:outline-none focus:ring-2 focus:ring-blue-500/50"
                            >
                                {gettingCreds ? 'Getting Credentials...' : 'Get AWS Credentials'}
                            </button>
                            {#if credsError}
                                <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-xs">{credsError}</div>
                            {/if}
                            {#if awsCreds}
                                <div class="bg-zinc-800 rounded p-3 space-y-2">
                                    <div class="flex justify-between text-xs">
                                        <span class="text-zinc-500">AccessKeyId</span>
                                        <span class="font-mono text-green-400">{awsCreds.AccessKeyId}</span>
                                    </div>
                                    <div class="flex items-center justify-between text-xs gap-2">
                                        <span class="text-zinc-500 shrink-0">SecretKey</span>
                                        <span class="font-mono text-zinc-400 flex-1 text-right">{showSecret ? awsCreds.SecretKey : '••••••••••••••'}</span>
                                        <button onclick={() => showSecret = !showSecret} class="text-xs text-orange-400 hover:text-orange-300 shrink-0">{showSecret ? 'Hide' : 'Show'}</button>
                                    </div>
                                    <div class="text-xs">
                                        <span class="text-zinc-500">SessionToken</span>
                                        <div class="font-mono text-zinc-400 text-xs truncate mt-0.5">{awsCreds.SessionToken}</div>
                                    </div>
                                    {#if awsCreds.Expiration}
                                        <div class="flex justify-between text-xs">
                                            <span class="text-zinc-500">Expiration</span>
                                            <span class="text-zinc-300">{new Date(awsCreds.Expiration).toLocaleString()}</span>
                                        </div>
                                    {/if}
                                </div>
                            {/if}
                        </div>
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

                                                {#if p['cognito:groups'] || p['cognito:roles']}
                                                    <div class="mt-3 border-t border-zinc-700 pt-3">
                                                        <h5 class="text-xs font-medium text-zinc-400 uppercase tracking-wider mb-2 flex items-center gap-2">
                                                            <span class="w-1.5 h-1.5 rounded-full bg-purple-500"></span>
                                                            Cognito Claims
                                                        </h5>

                                                        {#if p['cognito:groups']}
                                                            <div class="mb-2">
                                                                <span class="text-xs text-zinc-500">Groups</span>
                                                                <div class="flex flex-wrap gap-1 mt-1">
                                                                    {#each (p['cognito:groups'] as string[]) as group}
                                                                        <span class="px-2 py-0.5 bg-purple-900/30 text-purple-400 rounded text-xs">{group}</span>
                                                                    {/each}
                                                                </div>
                                                            </div>
                                                        {/if}

                                                        {#if p['cognito:roles']}
                                                            <div class="mb-2">
                                                                <span class="text-xs text-zinc-500">IAM Roles</span>
                                                                {#each (p['cognito:roles'] as string[]) as role}
                                                                    <div class="font-mono text-xs text-blue-400 mt-0.5 truncate">{role}</div>
                                                                {/each}
                                                            </div>
                                                        {/if}

                                                        {#if p['cognito:preferred_role']}
                                                            <div>
                                                                <span class="text-xs text-zinc-500">Preferred Role</span>
                                                                <div class="font-mono text-xs text-green-400 mt-0.5 bg-green-900/20 rounded px-2 py-1 inline-block">{p['cognito:preferred_role']}</div>
                                                            </div>
                                                        {/if}
                                                    </div>
                                                {/if}
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
