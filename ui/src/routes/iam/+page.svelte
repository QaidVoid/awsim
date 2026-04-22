<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listUsers, createUser, deleteUser,
        listRoles, createRole, deleteRole,
        listPolicies, listGroups,
        iamGetUser, iamListGroupsForUser, iamListUserPolicies,
        iamListAttachedUserPolicies, iamListAccessKeys, iamCreateAccessKey, iamDeleteAccessKey,
        iamListUserTags, iamTagUser, iamUntagUser,
        iamGetRole, iamListRolePolicies, iamListAttachedRolePolicies, iamUpdateAssumeRolePolicy,
        iamGetPolicy, iamListPolicyVersions, iamGetPolicyVersion, iamCreatePolicyVersion,
        iamDeletePolicyVersion, iamCreatePolicy, iamDeletePolicy,
        iamCreateGroup, iamDeleteGroup, iamGetGroup, iamListAttachedGroupPolicies,
        iamAddUserToGroup, iamRemoveUserFromGroup,
        iamAttachRolePolicy, iamDetachRolePolicy,
        iamAttachUserPolicy, iamDetachUserPolicy,
        iamAttachGroupPolicy, iamDetachGroupPolicy,
        iamGetAccountSummary, iamListAccountAliases, iamCreateAccountAlias, iamDeleteAccountAlias,
        iamGetAccountPasswordPolicy, iamUpdateAccountPasswordPolicy,
        type IamUser, type IamRole, type IamPolicy, type IamGroup,
        type IamAttachedPolicy, type IamAccessKey, type IamTag,
        type IamPolicyVersion, type IamAccountSummary, type IamPasswordPolicy,
    } from '$lib/aws';

    // ---- Top tabs ----
    let activeTab = $state<'users' | 'roles' | 'policies' | 'groups' | 'account'>('users');

    // ---- Users ----
    let users = $state<IamUser[]>([]);
    let usersLoading = $state(false);
    let usersError = $state<string | null>(null);
    let selectedUser = $state<IamUser | null>(null);
    let userSubTab = $state<'details' | 'policies' | 'groups' | 'accesskeys' | 'tags'>('details');
    let showCreateUser = $state(false);
    let newUserName = $state('');
    let creatingUser = $state(false);
    let createUserError = $state<string | null>(null);
    let confirmDeleteUser = $state<string | null>(null);

    // User detail
    let userInlinePolicies = $state<string[]>([]);
    let userAttachedPolicies = $state<IamAttachedPolicy[]>([]);
    let userGroups = $state<IamGroup[]>([]);
    let userAccessKeys = $state<IamAccessKey[]>([]);
    let userTags = $state<IamTag[]>([]);
    let userDetailLoading = $state(false);
    let newAccessKey = $state<{ accessKeyId: string; secretAccessKey: string } | null>(null);
    let creatingAccessKey = $state(false);
    let confirmDeleteKey = $state<string | null>(null);
    let attachUserPolicyArn = $state('');
    let attachingUserPolicy = $state(false);
    let confirmDetachUserPolicy = $state<string | null>(null);
    let addUserToGroupName = $state('');
    let addingUserToGroup = $state(false);
    let confirmRemoveUserFromGroup = $state<string | null>(null);
    let newTagKey = $state('');
    let newTagValue = $state('');
    let addingTag = $state(false);
    let confirmDeleteTag = $state<string | null>(null);
    let userActionError = $state<string | null>(null);
    let userActionSuccess = $state(false);

    // ---- Roles ----
    let roles = $state<IamRole[]>([]);
    let rolesLoading = $state(false);
    let rolesError = $state<string | null>(null);
    let selectedRole = $state<(IamRole & { assumeRolePolicyDocument?: string; description?: string; createDate?: string }) | null>(null);
    let roleSubTab = $state<'details' | 'trustpolicy' | 'policies' | 'tags'>('details');
    let showCreateRole = $state(false);
    let newRoleName = $state('');
    let newRoleDescription = $state('');
    let newRolePolicy = $state(JSON.stringify({
        Version: '2012-10-17',
        Statement: [{ Effect: 'Allow', Principal: { Service: 'lambda.amazonaws.com' }, Action: 'sts:AssumeRole' }]
    }, null, 2));
    let creatingRole = $state(false);
    let createRoleError = $state<string | null>(null);
    let confirmDeleteRole = $state<string | null>(null);

    // Role detail
    let roleInlinePolicies = $state<string[]>([]);
    let roleAttachedPolicies = $state<IamAttachedPolicy[]>([]);
    let roleTags = $state<IamTag[]>([]);
    let roleDetailLoading = $state(false);
    let editingTrustPolicy = $state(false);
    let editedTrustPolicy = $state('');
    let savingTrustPolicy = $state(false);
    let trustPolicySaved = $state(false);
    let attachRolePolicyArn = $state('');
    let attachingRolePolicy = $state(false);
    let confirmDetachRolePolicy = $state<string | null>(null);
    let roleActionError = $state<string | null>(null);

    // ---- Policies ----
    let policies = $state<IamPolicy[]>([]);
    let policiesLoading = $state(false);
    let policiesError = $state<string | null>(null);
    let selectedPolicy = $state<(IamPolicy & { defaultVersionId?: string; description?: string; createDate?: string }) | null>(null);
    let policySubTab = $state<'details' | 'document' | 'versions'>('details');
    let showCreatePolicy = $state(false);
    let newPolicyName = $state('');
    let newPolicyDescription = $state('');
    let newPolicyDocument = $state(JSON.stringify({
        Version: '2012-10-17',
        Statement: [{ Effect: 'Allow', Action: ['s3:GetObject'], Resource: '*' }]
    }, null, 2));
    let creatingPolicy = $state(false);
    let createPolicyError = $state<string | null>(null);
    let confirmDeletePolicy = $state<string | null>(null);

    // Policy detail
    let policyVersions = $state<IamPolicyVersion[]>([]);
    let policyDocument = $state('');
    let selectedPolicyVersion = $state<string | null>(null);
    let policyVersionDocument = $state('');
    let policyDetailLoading = $state(false);
    let newVersionDocument = $state('');
    let newVersionSetDefault = $state(true);
    let creatingVersion = $state(false);
    let showCreateVersion = $state(false);
    let confirmDeleteVersion = $state<string | null>(null);
    let policyActionError = $state<string | null>(null);

    // ---- Groups ----
    let groups = $state<IamGroup[]>([]);
    let groupsLoading = $state(false);
    let groupsError = $state<string | null>(null);
    let selectedGroup = $state<IamGroup | null>(null);
    let groupSubTab = $state<'members' | 'policies'>('members');
    let groupMembers = $state<IamUser[]>([]);
    let groupAttachedPolicies = $state<IamAttachedPolicy[]>([]);
    let groupDetailLoading = $state(false);
    let showCreateGroup = $state(false);
    let newGroupName = $state('');
    let creatingGroup = $state(false);
    let createGroupError = $state<string | null>(null);
    let confirmDeleteGroup = $state<string | null>(null);
    let addMemberName = $state('');
    let addingMember = $state(false);
    let confirmRemoveMember = $state<string | null>(null);
    let attachGroupPolicyArn = $state('');
    let attachingGroupPolicy = $state(false);
    let confirmDetachGroupPolicy = $state<string | null>(null);
    let groupActionError = $state<string | null>(null);

    // ---- Account ----
    let accountSummary = $state<IamAccountSummary | null>(null);
    let accountAliases = $state<string[]>([]);
    let passwordPolicy = $state<IamPasswordPolicy | null>(null);
    let accountLoading = $state(false);
    let accountError = $state<string | null>(null);
    let newAlias = $state('');
    let creatingAlias = $state(false);
    let confirmDeleteAlias = $state<string | null>(null);
    let editingPasswordPolicy = $state(false);
    let editedPasswordPolicy = $state<IamPasswordPolicy | null>(null);
    let savingPasswordPolicy = $state(false);
    let passwordPolicySaved = $state(false);

    // ---- Helpers ----
    function formatDate(iso: string): string {
        if (!iso) return '—';
        try { return new Date(iso).toLocaleString(); } catch { return iso; }
    }

    function isServiceLinkedRole(role: IamRole): boolean {
        return role.arn.includes(':role/aws-service-role/');
    }

    function copyToClipboard(text: string) {
        navigator.clipboard.writeText(text).catch(() => {});
    }

    function formatJson(doc: string): string {
        try { return JSON.stringify(JSON.parse(doc), null, 2); } catch { return doc; }
    }

    async function flashSuccess(setter: (v: boolean) => void) {
        setter(true);
        await new Promise((r) => setTimeout(r, 2000));
        setter(false);
    }

    // ---- Users ----
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
            if (selectedUser?.userName === userName) selectedUser = null;
            await loadUsers();
        } catch (e) {
            usersError = e instanceof Error ? e.message : 'Failed to delete user';
        }
    }

    async function selectUser(user: IamUser) {
        selectedUser = user;
        userSubTab = 'details';
        userActionError = null;
        newAccessKey = null;
        await loadUserDetail(user.userName);
    }

    async function loadUserDetail(userName: string) {
        userDetailLoading = true;
        try {
            const [pols, attached, grps, keys, tags] = await Promise.all([
                iamListUserPolicies(userName).catch(() => ({ policyNames: [] })),
                iamListAttachedUserPolicies(userName).catch(() => ({ policies: [] })),
                iamListGroupsForUser(userName).catch(() => ({ groups: [] })),
                iamListAccessKeys(userName).catch(() => ({ accessKeys: [] })),
                iamListUserTags(userName).catch(() => ({ tags: [] })),
            ]);
            userInlinePolicies = pols.policyNames;
            userAttachedPolicies = attached.policies;
            userGroups = grps.groups;
            userAccessKeys = keys.accessKeys;
            userTags = tags.tags;
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to load user details';
        } finally {
            userDetailLoading = false;
        }
    }

    async function handleCreateAccessKey() {
        if (!selectedUser) return;
        creatingAccessKey = true;
        userActionError = null;
        try {
            const key = await iamCreateAccessKey(selectedUser.userName);
            newAccessKey = key;
            const keys = await iamListAccessKeys(selectedUser.userName);
            userAccessKeys = keys.accessKeys;
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to create access key';
        } finally {
            creatingAccessKey = false;
        }
    }

    async function handleDeleteAccessKey(keyId: string) {
        if (!selectedUser) return;
        try {
            await iamDeleteAccessKey(selectedUser.userName, keyId);
            confirmDeleteKey = null;
            const keys = await iamListAccessKeys(selectedUser.userName);
            userAccessKeys = keys.accessKeys;
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to delete access key';
        }
    }

    async function handleAttachUserPolicy() {
        if (!selectedUser || !attachUserPolicyArn.trim()) return;
        attachingUserPolicy = true;
        userActionError = null;
        try {
            await iamAttachUserPolicy(selectedUser.userName, attachUserPolicyArn.trim());
            attachUserPolicyArn = '';
            const attached = await iamListAttachedUserPolicies(selectedUser.userName);
            userAttachedPolicies = attached.policies;
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to attach policy';
        } finally {
            attachingUserPolicy = false;
        }
    }

    async function handleDetachUserPolicy(policyArn: string) {
        if (!selectedUser) return;
        try {
            await iamDetachUserPolicy(selectedUser.userName, policyArn);
            confirmDetachUserPolicy = null;
            const attached = await iamListAttachedUserPolicies(selectedUser.userName);
            userAttachedPolicies = attached.policies;
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to detach policy';
        }
    }

    async function handleAddUserToGroup() {
        if (!selectedUser || !addUserToGroupName.trim()) return;
        addingUserToGroup = true;
        userActionError = null;
        try {
            await iamAddUserToGroup(selectedUser.userName, addUserToGroupName.trim());
            addUserToGroupName = '';
            const grps = await iamListGroupsForUser(selectedUser.userName);
            userGroups = grps.groups;
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to add to group';
        } finally {
            addingUserToGroup = false;
        }
    }

    async function handleRemoveUserFromGroup(groupName: string) {
        if (!selectedUser) return;
        try {
            await iamRemoveUserFromGroup(selectedUser.userName, groupName);
            confirmRemoveUserFromGroup = null;
            const grps = await iamListGroupsForUser(selectedUser.userName);
            userGroups = grps.groups;
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to remove from group';
        }
    }

    async function handleAddTag() {
        if (!selectedUser || !newTagKey.trim()) return;
        addingTag = true;
        userActionError = null;
        try {
            await iamTagUser(selectedUser.userName, [{ key: newTagKey.trim(), value: newTagValue.trim() }]);
            newTagKey = '';
            newTagValue = '';
            const tags = await iamListUserTags(selectedUser.userName);
            userTags = tags.tags;
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to add tag';
        } finally {
            addingTag = false;
        }
    }

    async function handleDeleteTag(key: string) {
        if (!selectedUser) return;
        try {
            await iamUntagUser(selectedUser.userName, [key]);
            confirmDeleteTag = null;
            const tags = await iamListUserTags(selectedUser.userName);
            userTags = tags.tags;
        } catch (e) {
            userActionError = e instanceof Error ? e.message : 'Failed to delete tag';
        }
    }

    // ---- Roles ----
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
            await createRole(newRoleName.trim(), newRolePolicy, newRoleDescription || undefined);
            newRoleName = '';
            newRoleDescription = '';
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
            if (selectedRole?.roleName === roleName) selectedRole = null;
            await loadRoles();
        } catch (e) {
            rolesError = e instanceof Error ? e.message : 'Failed to delete role';
        }
    }

    async function selectRole(role: IamRole) {
        roleSubTab = 'details';
        roleActionError = null;
        editingTrustPolicy = false;
        roleDetailLoading = true;
        try {
            const [detail, inlinePols, attachedPols] = await Promise.all([
                iamGetRole(role.roleName).catch(() => ({ ...role, assumeRolePolicyDocument: '', description: undefined, createDate: undefined })),
                iamListRolePolicies(role.roleName).catch(() => ({ policyNames: [] })),
                iamListAttachedRolePolicies(role.roleName).catch(() => ({ policies: [] })),
            ]);
            selectedRole = detail;
            editedTrustPolicy = formatJson(detail.assumeRolePolicyDocument || '{}');
            roleInlinePolicies = inlinePols.policyNames;
            roleAttachedPolicies = attachedPols.policies;
        } catch (e) {
            roleActionError = e instanceof Error ? e.message : 'Failed to load role details';
            selectedRole = role;
        } finally {
            roleDetailLoading = false;
        }
    }

    async function handleSaveTrustPolicy() {
        if (!selectedRole) return;
        savingTrustPolicy = true;
        roleActionError = null;
        try {
            await iamUpdateAssumeRolePolicy(selectedRole.roleName, editedTrustPolicy);
            editingTrustPolicy = false;
            flashSuccess((v) => { trustPolicySaved = v; });
        } catch (e) {
            roleActionError = e instanceof Error ? e.message : 'Failed to update trust policy';
        } finally {
            savingTrustPolicy = false;
        }
    }

    async function handleAttachRolePolicy() {
        if (!selectedRole || !attachRolePolicyArn.trim()) return;
        attachingRolePolicy = true;
        roleActionError = null;
        try {
            await iamAttachRolePolicy(selectedRole.roleName, attachRolePolicyArn.trim());
            attachRolePolicyArn = '';
            const attached = await iamListAttachedRolePolicies(selectedRole.roleName);
            roleAttachedPolicies = attached.policies;
        } catch (e) {
            roleActionError = e instanceof Error ? e.message : 'Failed to attach policy';
        } finally {
            attachingRolePolicy = false;
        }
    }

    async function handleDetachRolePolicy(policyArn: string) {
        if (!selectedRole) return;
        try {
            await iamDetachRolePolicy(selectedRole.roleName, policyArn);
            confirmDetachRolePolicy = null;
            const attached = await iamListAttachedRolePolicies(selectedRole.roleName);
            roleAttachedPolicies = attached.policies;
        } catch (e) {
            roleActionError = e instanceof Error ? e.message : 'Failed to detach policy';
        }
    }

    // ---- Policies ----
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

    async function handleCreatePolicy() {
        if (!newPolicyName.trim() || !newPolicyDocument.trim()) return;
        creatingPolicy = true;
        createPolicyError = null;
        try {
            await iamCreatePolicy(newPolicyName.trim(), newPolicyDocument, newPolicyDescription || undefined);
            newPolicyName = '';
            newPolicyDescription = '';
            showCreatePolicy = false;
            await loadPolicies();
        } catch (e) {
            createPolicyError = e instanceof Error ? e.message : 'Failed to create policy';
        } finally {
            creatingPolicy = false;
        }
    }

    async function handleDeletePolicy(arn: string) {
        try {
            await iamDeletePolicy(arn);
            confirmDeletePolicy = null;
            if (selectedPolicy?.arn === arn) selectedPolicy = null;
            await loadPolicies();
        } catch (e) {
            policiesError = e instanceof Error ? e.message : 'Failed to delete policy';
        }
    }

    async function selectPolicy(policy: IamPolicy) {
        policySubTab = 'details';
        policyActionError = null;
        policyDetailLoading = true;
        selectedPolicy = policy;
        policyDocument = '';
        policyVersions = [];
        selectedPolicyVersion = null;
        policyVersionDocument = '';
        try {
            const [detail, versions] = await Promise.all([
                iamGetPolicy(policy.arn).catch(() => ({ ...policy, defaultVersionId: 'v1', description: undefined, createDate: undefined })),
                iamListPolicyVersions(policy.arn).catch(() => ({ versions: [] })),
            ]);
            selectedPolicy = detail;
            policyVersions = versions.versions;
            if (detail.defaultVersionId) {
                const docData = await iamGetPolicyVersion(policy.arn, detail.defaultVersionId).catch(() => ({ document: '', isDefaultVersion: true }));
                policyDocument = formatJson(docData.document);
            }
        } catch (e) {
            policyActionError = e instanceof Error ? e.message : 'Failed to load policy details';
        } finally {
            policyDetailLoading = false;
        }
    }

    async function loadPolicyVersion(versionId: string) {
        if (!selectedPolicy) return;
        selectedPolicyVersion = versionId;
        try {
            const data = await iamGetPolicyVersion(selectedPolicy.arn, versionId);
            policyVersionDocument = formatJson(data.document);
        } catch (e) {
            policyVersionDocument = 'Failed to load version document';
        }
    }

    async function handleCreateVersion() {
        if (!selectedPolicy || !newVersionDocument.trim()) return;
        creatingVersion = true;
        policyActionError = null;
        try {
            await iamCreatePolicyVersion(selectedPolicy.arn, newVersionDocument, newVersionSetDefault);
            showCreateVersion = false;
            newVersionDocument = '';
            const [versions, detail] = await Promise.all([
                iamListPolicyVersions(selectedPolicy.arn),
                iamGetPolicy(selectedPolicy.arn),
            ]);
            policyVersions = versions.versions;
            selectedPolicy = detail;
            if (detail.defaultVersionId) {
                const docData = await iamGetPolicyVersion(selectedPolicy.arn, detail.defaultVersionId);
                policyDocument = formatJson(docData.document);
            }
        } catch (e) {
            policyActionError = e instanceof Error ? e.message : 'Failed to create version';
        } finally {
            creatingVersion = false;
        }
    }

    async function handleDeleteVersion(versionId: string) {
        if (!selectedPolicy) return;
        try {
            await iamDeletePolicyVersion(selectedPolicy.arn, versionId);
            confirmDeleteVersion = null;
            const versions = await iamListPolicyVersions(selectedPolicy.arn);
            policyVersions = versions.versions;
        } catch (e) {
            policyActionError = e instanceof Error ? e.message : 'Failed to delete version';
        }
    }

    // ---- Groups ----
    async function loadGroups() {
        groupsLoading = true;
        groupsError = null;
        try {
            const data = await listGroups();
            groups = data.groups;
        } catch (e) {
            groupsError = e instanceof Error ? e.message : 'Failed to load groups';
        } finally {
            groupsLoading = false;
        }
    }

    async function handleCreateGroup() {
        if (!newGroupName.trim()) return;
        creatingGroup = true;
        createGroupError = null;
        try {
            await iamCreateGroup(newGroupName.trim());
            newGroupName = '';
            showCreateGroup = false;
            await loadGroups();
        } catch (e) {
            createGroupError = e instanceof Error ? e.message : 'Failed to create group';
        } finally {
            creatingGroup = false;
        }
    }

    async function handleDeleteGroup(name: string) {
        try {
            await iamDeleteGroup(name);
            confirmDeleteGroup = null;
            if (selectedGroup?.groupName === name) selectedGroup = null;
            await loadGroups();
        } catch (e) {
            groupsError = e instanceof Error ? e.message : 'Failed to delete group';
        }
    }

    async function selectGroup(group: IamGroup) {
        selectedGroup = group;
        groupSubTab = 'members';
        groupActionError = null;
        groupDetailLoading = true;
        try {
            const [detail, attachedPols] = await Promise.all([
                iamGetGroup(group.groupName).catch(() => ({ group, users: [] })),
                iamListAttachedGroupPolicies(group.groupName).catch(() => ({ policies: [] })),
            ]);
            groupMembers = detail.users;
            groupAttachedPolicies = attachedPols.policies;
        } catch (e) {
            groupActionError = e instanceof Error ? e.message : 'Failed to load group details';
        } finally {
            groupDetailLoading = false;
        }
    }

    async function handleAddMember() {
        if (!selectedGroup || !addMemberName.trim()) return;
        addingMember = true;
        groupActionError = null;
        try {
            await iamAddUserToGroup(addMemberName.trim(), selectedGroup.groupName);
            addMemberName = '';
            const detail = await iamGetGroup(selectedGroup.groupName);
            groupMembers = detail.users;
        } catch (e) {
            groupActionError = e instanceof Error ? e.message : 'Failed to add member';
        } finally {
            addingMember = false;
        }
    }

    async function handleRemoveMember(userName: string) {
        if (!selectedGroup) return;
        try {
            await iamRemoveUserFromGroup(userName, selectedGroup.groupName);
            confirmRemoveMember = null;
            const detail = await iamGetGroup(selectedGroup.groupName);
            groupMembers = detail.users;
        } catch (e) {
            groupActionError = e instanceof Error ? e.message : 'Failed to remove member';
        }
    }

    async function handleAttachGroupPolicy() {
        if (!selectedGroup || !attachGroupPolicyArn.trim()) return;
        attachingGroupPolicy = true;
        groupActionError = null;
        try {
            await iamAttachGroupPolicy(selectedGroup.groupName, attachGroupPolicyArn.trim());
            attachGroupPolicyArn = '';
            const attached = await iamListAttachedGroupPolicies(selectedGroup.groupName);
            groupAttachedPolicies = attached.policies;
        } catch (e) {
            groupActionError = e instanceof Error ? e.message : 'Failed to attach policy';
        } finally {
            attachingGroupPolicy = false;
        }
    }

    async function handleDetachGroupPolicy(policyArn: string) {
        if (!selectedGroup) return;
        try {
            await iamDetachGroupPolicy(selectedGroup.groupName, policyArn);
            confirmDetachGroupPolicy = null;
            const attached = await iamListAttachedGroupPolicies(selectedGroup.groupName);
            groupAttachedPolicies = attached.policies;
        } catch (e) {
            groupActionError = e instanceof Error ? e.message : 'Failed to detach policy';
        }
    }

    // ---- Account ----
    async function loadAccount() {
        accountLoading = true;
        accountError = null;
        try {
            const [summary, aliases, pwPolicy] = await Promise.all([
                iamGetAccountSummary().catch(() => null),
                iamListAccountAliases().catch(() => ({ aliases: [] })),
                iamGetAccountPasswordPolicy().catch(() => null),
            ]);
            accountSummary = summary;
            accountAliases = aliases.aliases;
            passwordPolicy = pwPolicy;
            if (pwPolicy) editedPasswordPolicy = { ...pwPolicy };
        } catch (e) {
            accountError = e instanceof Error ? e.message : 'Failed to load account info';
        } finally {
            accountLoading = false;
        }
    }

    async function handleCreateAlias() {
        if (!newAlias.trim()) return;
        creatingAlias = true;
        accountError = null;
        try {
            await iamCreateAccountAlias(newAlias.trim());
            newAlias = '';
            const aliases = await iamListAccountAliases();
            accountAliases = aliases.aliases;
        } catch (e) {
            accountError = e instanceof Error ? e.message : 'Failed to create alias';
        } finally {
            creatingAlias = false;
        }
    }

    async function handleDeleteAlias(alias: string) {
        try {
            await iamDeleteAccountAlias(alias);
            confirmDeleteAlias = null;
            const aliases = await iamListAccountAliases();
            accountAliases = aliases.aliases;
        } catch (e) {
            accountError = e instanceof Error ? e.message : 'Failed to delete alias';
        }
    }

    async function handleSavePasswordPolicy() {
        if (!editedPasswordPolicy) return;
        savingPasswordPolicy = true;
        accountError = null;
        try {
            await iamUpdateAccountPasswordPolicy(editedPasswordPolicy);
            passwordPolicy = { ...editedPasswordPolicy };
            editingPasswordPolicy = false;
            flashSuccess((v) => { passwordPolicySaved = v; });
        } catch (e) {
            accountError = e instanceof Error ? e.message : 'Failed to update password policy';
        } finally {
            savingPasswordPolicy = false;
        }
    }

    // ---- Tab switching ----
    function switchTab(tab: typeof activeTab) {
        activeTab = tab;
        if (tab === 'users' && users.length === 0 && !usersLoading) loadUsers();
        else if (tab === 'roles' && roles.length === 0 && !rolesLoading) loadRoles();
        else if (tab === 'policies' && policies.length === 0 && !policiesLoading) loadPolicies();
        else if (tab === 'groups' && groups.length === 0 && !groupsLoading) loadGroups();
        else if (tab === 'account' && !accountSummary && !accountLoading) loadAccount();
    }

    onMount(() => loadUsers());
</script>

<div class="p-6 h-full flex flex-col">
    <!-- Header -->
    <div class="flex items-center justify-between mb-4">
        <div>
            <h1 class="text-2xl font-bold">IAM</h1>
            <p class="text-zinc-500 mt-0.5 text-sm">Identity &amp; Access Management</p>
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
        {:else if activeTab === 'policies'}
            <button
                onclick={() => { showCreatePolicy = !showCreatePolicy; createPolicyError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Policy
            </button>
        {:else if activeTab === 'groups'}
            <button
                onclick={() => { showCreateGroup = !showCreateGroup; createGroupError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Group
            </button>
        {/if}
    </div>

    <!-- Top tabs -->
    <div class="flex gap-1 mb-4">
        {#each [
            { id: 'users', label: 'Users', count: users.length },
            { id: 'roles', label: 'Roles', count: roles.length },
            { id: 'policies', label: 'Policies', count: policies.length },
            { id: 'groups', label: 'Groups', count: groups.length },
            { id: 'account', label: 'Account', count: null },
        ] as tab}
            <button
                onclick={() => switchTab(tab.id as typeof activeTab)}
                class="px-4 py-1.5 rounded-full text-sm font-medium transition-colors flex items-center gap-1.5 {activeTab === tab.id ? 'bg-orange-600 text-white' : 'bg-zinc-800 text-zinc-400 hover:bg-zinc-700 hover:text-zinc-200'}"
            >
                {tab.label}
                {#if tab.count !== null && tab.count > 0}
                    <span class="text-xs px-1.5 py-0.5 rounded-full {activeTab === tab.id ? 'bg-orange-800 text-orange-200' : 'bg-zinc-700 text-zinc-400'}">{tab.count}</span>
                {/if}
            </button>
        {/each}
    </div>

    <!-- ============ USERS TAB ============ -->
    {#if activeTab === 'users'}
        <!-- Create User form -->
        {#if showCreateUser}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create IAM User</h3>
                {#if createUserError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createUserError}</div>
                {/if}
                <div class="flex gap-2">
                    <input
                        type="text"
                        bind:value={newUserName}
                        onkeydown={(e) => e.key === 'Enter' && handleCreateUser()}
                        class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        placeholder="username"
                    />
                    <button
                        onclick={handleCreateUser}
                        disabled={creatingUser || !newUserName.trim()}
                        class="px-4 py-2 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                    >
                        {creatingUser ? 'Creating...' : 'Create'}
                    </button>
                    <button
                        onclick={() => { showCreateUser = false; createUserError = null; newUserName = ''; }}
                        class="px-3 py-2 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        {/if}

        <div class="flex gap-4 flex-1 min-h-0">
            <!-- User list -->
            <div class="w-72 flex-shrink-0 flex flex-col">
                {#if usersLoading}
                    <div class="text-zinc-500 text-sm">Loading users...</div>
                {:else if usersError}
                    <div class="bg-red-900/20 border border-red-800 rounded-lg p-3 text-red-400 text-sm">{usersError}</div>
                {:else if users.length === 0}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-6 text-center">
                        <p class="text-zinc-500 text-sm">No IAM users yet.</p>
                        <button onclick={() => showCreateUser = true} class="mt-2 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                            Create User
                        </button>
                    </div>
                {:else}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden flex-1">
                        {#each users as user}
                            <div
                                class="border-b border-zinc-800/50 px-3 py-2.5 cursor-pointer transition-colors {selectedUser?.userName === user.userName ? 'bg-orange-900/20 border-l-2 border-l-orange-500' : 'hover:bg-zinc-800/50'}"
                                onclick={() => selectUser(user)}
                            >
                                <div class="flex items-center justify-between">
                                    <span class="font-mono text-sm text-orange-400">{user.userName}</span>
                                    {#if confirmDeleteUser === user.userName}
                                        <div class="flex items-center gap-1" onclick={(e) => e.stopPropagation()}>
                                            <button onclick={() => handleDeleteUser(user.userName)} class="px-1.5 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteUser = null} class="px-1.5 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button
                                            onclick={(e) => { e.stopPropagation(); confirmDeleteUser = user.userName; }}
                                            class="text-red-400 hover:text-red-300 text-xs px-1.5 py-0.5 rounded hover:bg-red-900/30 transition-colors"
                                        >
                                            Delete
                                        </button>
                                    {/if}
                                </div>
                                <div class="text-xs text-zinc-500 mt-0.5">{formatDate(user.createDate)}</div>
                            </div>
                        {/each}
                    </div>
                {/if}
            </div>

            <!-- User detail panel -->
            {#if selectedUser}
                <div class="flex-1 min-w-0 bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden flex flex-col">
                    <div class="px-4 py-3 border-b border-zinc-800 flex items-center gap-2">
                        <span class="font-semibold">{selectedUser.userName}</span>
                        <span class="text-zinc-500 text-xs">IAM User</span>
                    </div>

                    <!-- User sub-tabs -->
                    <div class="flex border-b border-zinc-800 px-4">
                        {#each [
                            { id: 'details', label: 'Details' },
                            { id: 'policies', label: 'Policies' },
                            { id: 'groups', label: 'Groups' },
                            { id: 'accesskeys', label: 'Access Keys' },
                            { id: 'tags', label: 'Tags' },
                        ] as sub}
                            <button
                                onclick={() => userSubTab = sub.id as typeof userSubTab}
                                class="px-3 py-2 text-sm border-b-2 transition-colors {userSubTab === sub.id ? 'border-orange-400 text-orange-400' : 'border-transparent text-zinc-500 hover:text-zinc-300'}"
                            >
                                {sub.label}
                            </button>
                        {/each}
                    </div>

                    {#if userActionError}
                        <div class="mx-4 mt-3 bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm">{userActionError}</div>
                    {/if}

                    {#if userDetailLoading}
                        <div class="p-4 text-zinc-500 text-sm">Loading...</div>
                    {:else}
                        <div class="p-4 overflow-y-auto flex-1">
                            <!-- Details sub-tab -->
                            {#if userSubTab === 'details'}
                                <div class="space-y-3">
                                    <div class="bg-zinc-800/50 rounded-lg p-3 space-y-2">
                                        <div class="flex items-center justify-between">
                                            <span class="text-zinc-400 text-xs uppercase tracking-wide">ARN</span>
                                            <button onclick={() => copyToClipboard(selectedUser!.arn)} class="text-xs text-zinc-500 hover:text-zinc-300">Copy</button>
                                        </div>
                                        <p class="font-mono text-xs text-zinc-200 break-all">{selectedUser.arn}</p>
                                    </div>
                                    <div class="bg-zinc-800/50 rounded-lg p-3 space-y-2">
                                        <span class="text-zinc-400 text-xs uppercase tracking-wide">User ID</span>
                                        <p class="font-mono text-xs text-zinc-200">{selectedUser.userId}</p>
                                    </div>
                                    <div class="bg-zinc-800/50 rounded-lg p-3 space-y-2">
                                        <span class="text-zinc-400 text-xs uppercase tracking-wide">Created</span>
                                        <p class="text-sm text-zinc-200">{formatDate(selectedUser.createDate)}</p>
                                    </div>
                                </div>

                            <!-- Policies sub-tab -->
                            {:else if userSubTab === 'policies'}
                                <div class="space-y-4">
                                    <!-- Inline policies -->
                                    <div>
                                        <h4 class="text-sm font-semibold mb-2 text-zinc-300">Inline Policies</h4>
                                        {#if userInlinePolicies.length === 0}
                                            <p class="text-zinc-500 text-sm">No inline policies.</p>
                                        {:else}
                                            <div class="space-y-1">
                                                {#each userInlinePolicies as pol}
                                                    <div class="bg-zinc-800 rounded px-3 py-2 text-sm font-mono text-zinc-300">{pol}</div>
                                                {/each}
                                            </div>
                                        {/if}
                                    </div>

                                    <!-- Attached managed policies -->
                                    <div>
                                        <h4 class="text-sm font-semibold mb-2 text-zinc-300">Attached Managed Policies</h4>
                                        {#if userAttachedPolicies.length === 0}
                                            <p class="text-zinc-500 text-sm">No attached policies.</p>
                                        {:else}
                                            <div class="space-y-1">
                                                {#each userAttachedPolicies as pol}
                                                    <div class="bg-zinc-800 rounded px-3 py-2 flex items-center justify-between">
                                                        <div>
                                                            <div class="text-sm font-mono text-zinc-200">{pol.policyName}</div>
                                                            <div class="text-xs text-zinc-500 font-mono">{pol.policyArn}</div>
                                                        </div>
                                                        {#if confirmDetachUserPolicy === pol.policyArn}
                                                            <div class="flex gap-1">
                                                                <button onclick={() => handleDetachUserPolicy(pol.policyArn)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                                                <button onclick={() => confirmDetachUserPolicy = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                                            </div>
                                                        {:else}
                                                            <button onclick={() => confirmDetachUserPolicy = pol.policyArn} class="text-xs text-red-400 hover:text-red-300 px-2 py-0.5 rounded hover:bg-red-900/30 transition-colors">Detach</button>
                                                        {/if}
                                                    </div>
                                                {/each}
                                            </div>
                                        {/if}
                                        <!-- Attach policy -->
                                        <div class="mt-3 flex gap-2">
                                            <select
                                                bind:value={attachUserPolicyArn}
                                                class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500"
                                            >
                                                <option value="">-- Select policy to attach --</option>
                                                {#each policies as pol}
                                                    <option value={pol.arn}>{pol.policyName}</option>
                                                {/each}
                                            </select>
                                            <button
                                                onclick={handleAttachUserPolicy}
                                                disabled={attachingUserPolicy || !attachUserPolicyArn}
                                                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm transition-colors"
                                            >
                                                {attachingUserPolicy ? 'Attaching...' : 'Attach'}
                                            </button>
                                        </div>
                                    </div>
                                </div>

                            <!-- Groups sub-tab -->
                            {:else if userSubTab === 'groups'}
                                <div class="space-y-3">
                                    {#if userGroups.length === 0}
                                        <p class="text-zinc-500 text-sm">Not a member of any groups.</p>
                                    {:else}
                                        <div class="space-y-1">
                                            {#each userGroups as grp}
                                                <div class="bg-zinc-800 rounded px-3 py-2 flex items-center justify-between">
                                                    <span class="font-mono text-sm text-zinc-200">{grp.groupName}</span>
                                                    {#if confirmRemoveUserFromGroup === grp.groupName}
                                                        <div class="flex gap-1">
                                                            <button onclick={() => handleRemoveUserFromGroup(grp.groupName)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                                            <button onclick={() => confirmRemoveUserFromGroup = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                                        </div>
                                                    {:else}
                                                        <button onclick={() => confirmRemoveUserFromGroup = grp.groupName} class="text-xs text-red-400 hover:text-red-300 px-2 py-0.5 rounded hover:bg-red-900/30 transition-colors">Remove</button>
                                                    {/if}
                                                </div>
                                            {/each}
                                        </div>
                                    {/if}
                                    <div class="flex gap-2">
                                        <select
                                            bind:value={addUserToGroupName}
                                            class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500"
                                        >
                                            <option value="">-- Add to group --</option>
                                            {#each groups as grp}
                                                <option value={grp.groupName}>{grp.groupName}</option>
                                            {/each}
                                        </select>
                                        <button
                                            onclick={handleAddUserToGroup}
                                            disabled={addingUserToGroup || !addUserToGroupName}
                                            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm transition-colors"
                                        >
                                            {addingUserToGroup ? 'Adding...' : 'Add'}
                                        </button>
                                    </div>
                                </div>

                            <!-- Access Keys sub-tab -->
                            {:else if userSubTab === 'accesskeys'}
                                <div class="space-y-3">
                                    {#if newAccessKey}
                                        <div class="bg-green-900/20 border border-green-700 rounded-lg p-3">
                                            <p class="text-green-400 text-sm font-semibold mb-2">New Access Key — save the secret now, it won't be shown again.</p>
                                            <div class="space-y-1">
                                                <div class="flex items-center gap-2">
                                                    <span class="text-zinc-400 text-xs w-32">Access Key ID</span>
                                                    <span class="font-mono text-xs text-zinc-200">{newAccessKey.accessKeyId}</span>
                                                    <button onclick={() => copyToClipboard(newAccessKey!.accessKeyId)} class="text-xs text-zinc-500 hover:text-zinc-300">Copy</button>
                                                </div>
                                                <div class="flex items-center gap-2">
                                                    <span class="text-zinc-400 text-xs w-32">Secret Access Key</span>
                                                    <span class="font-mono text-xs text-zinc-200 break-all">{newAccessKey.secretAccessKey}</span>
                                                    <button onclick={() => copyToClipboard(newAccessKey!.secretAccessKey)} class="text-xs text-zinc-500 hover:text-zinc-300">Copy</button>
                                                </div>
                                            </div>
                                            <button onclick={() => newAccessKey = null} class="mt-2 text-xs text-zinc-400 hover:text-zinc-300">Dismiss</button>
                                        </div>
                                    {/if}
                                    {#if userAccessKeys.length === 0}
                                        <p class="text-zinc-500 text-sm">No access keys.</p>
                                    {:else}
                                        <div class="space-y-1">
                                            {#each userAccessKeys as key}
                                                <div class="bg-zinc-800 rounded px-3 py-2 flex items-center justify-between">
                                                    <div>
                                                        <span class="font-mono text-sm text-zinc-200">{key.accessKeyId}</span>
                                                        <div class="flex items-center gap-2 mt-0.5">
                                                            <span class="text-xs px-1.5 py-0.5 rounded-full {key.status === 'Active' ? 'bg-green-900/30 text-green-400' : 'bg-yellow-900/30 text-yellow-400'}">{key.status}</span>
                                                            <span class="text-xs text-zinc-500">{formatDate(key.createDate)}</span>
                                                        </div>
                                                    </div>
                                                    {#if confirmDeleteKey === key.accessKeyId}
                                                        <div class="flex gap-1">
                                                            <button onclick={() => handleDeleteAccessKey(key.accessKeyId)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                                            <button onclick={() => confirmDeleteKey = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                                        </div>
                                                    {:else}
                                                        <button onclick={() => confirmDeleteKey = key.accessKeyId} class="text-xs text-red-400 hover:text-red-300 px-2 py-0.5 rounded hover:bg-red-900/30 transition-colors">Delete</button>
                                                    {/if}
                                                </div>
                                            {/each}
                                        </div>
                                    {/if}
                                    <button
                                        onclick={handleCreateAccessKey}
                                        disabled={creatingAccessKey}
                                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm font-medium transition-colors"
                                    >
                                        {creatingAccessKey ? 'Creating...' : 'Create Access Key'}
                                    </button>
                                </div>

                            <!-- Tags sub-tab -->
                            {:else if userSubTab === 'tags'}
                                <div class="space-y-3">
                                    {#if userTags.length === 0}
                                        <p class="text-zinc-500 text-sm">No tags.</p>
                                    {:else}
                                        <div class="border border-zinc-700 rounded-lg overflow-hidden">
                                            <table class="w-full text-sm">
                                                <thead>
                                                    <tr class="bg-zinc-800 text-zinc-400 text-xs">
                                                        <th class="px-3 py-2 text-left">Key</th>
                                                        <th class="px-3 py-2 text-left">Value</th>
                                                        <th class="px-3 py-2"></th>
                                                    </tr>
                                                </thead>
                                                <tbody>
                                                    {#each userTags as tag}
                                                        <tr class="border-t border-zinc-700/50">
                                                            <td class="px-3 py-2 font-mono text-zinc-200">{tag.key}</td>
                                                            <td class="px-3 py-2 text-zinc-400">{tag.value}</td>
                                                            <td class="px-3 py-2">
                                                                {#if confirmDeleteTag === tag.key}
                                                                    <div class="flex gap-1">
                                                                        <button onclick={() => handleDeleteTag(tag.key)} class="px-1.5 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                                                        <button onclick={() => confirmDeleteTag = null} class="px-1.5 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                                                    </div>
                                                                {:else}
                                                                    <button onclick={() => confirmDeleteTag = tag.key} class="text-xs text-red-400 hover:text-red-300 px-1.5 py-0.5 rounded hover:bg-red-900/30 transition-colors">Delete</button>
                                                                {/if}
                                                            </td>
                                                        </tr>
                                                    {/each}
                                                </tbody>
                                            </table>
                                        </div>
                                    {/if}
                                    <div class="flex gap-2">
                                        <input
                                            type="text"
                                            bind:value={newTagKey}
                                            placeholder="Key"
                                            class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500"
                                        />
                                        <input
                                            type="text"
                                            bind:value={newTagValue}
                                            placeholder="Value"
                                            class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500"
                                        />
                                        <button
                                            onclick={handleAddTag}
                                            disabled={addingTag || !newTagKey.trim()}
                                            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm transition-colors"
                                        >
                                            {addingTag ? 'Adding...' : 'Add Tag'}
                                        </button>
                                    </div>
                                </div>
                            {/if}
                        </div>
                    {/if}
                </div>
            {:else}
                <div class="flex-1 flex items-center justify-center text-zinc-600">
                    <p>Select a user to view details</p>
                </div>
            {/if}
        </div>
    {/if}

    <!-- ============ ROLES TAB ============ -->
    {#if activeTab === 'roles'}
        {#if showCreateRole}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create IAM Role</h3>
                {#if createRoleError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createRoleError}</div>
                {/if}
                <div class="grid grid-cols-2 gap-3 mb-3">
                    <div>
                        <label class="block text-xs text-zinc-400 mb-1">Role Name</label>
                        <input type="text" bind:value={newRoleName} placeholder="my-role"
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" />
                    </div>
                    <div>
                        <label class="block text-xs text-zinc-400 mb-1">Description (optional)</label>
                        <input type="text" bind:value={newRoleDescription} placeholder="Role description"
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" />
                    </div>
                </div>
                <div class="mb-3">
                    <label class="block text-xs text-zinc-400 mb-1">Trust Policy (JSON)</label>
                    <textarea bind:value={newRolePolicy} rows="8"
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y"></textarea>
                </div>
                <div class="flex gap-2">
                    <button onclick={handleCreateRole} disabled={creatingRole || !newRoleName.trim()}
                        class="px-4 py-2 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm font-medium transition-colors">
                        {creatingRole ? 'Creating...' : 'Create Role'}
                    </button>
                    <button onclick={() => { showCreateRole = false; createRoleError = null; }}
                        class="px-3 py-2 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors">Cancel</button>
                </div>
            </div>
        {/if}

        <div class="flex gap-4 flex-1 min-h-0">
            <!-- Role list -->
            <div class="w-72 flex-shrink-0 flex flex-col">
                {#if rolesLoading}
                    <div class="text-zinc-500 text-sm">Loading roles...</div>
                {:else if rolesError}
                    <div class="bg-red-900/20 border border-red-800 rounded-lg p-3 text-red-400 text-sm">{rolesError}</div>
                {:else if roles.length === 0}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-6 text-center">
                        <p class="text-zinc-500 text-sm">No IAM roles yet.</p>
                        <button onclick={() => showCreateRole = true} class="mt-2 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                            Create Role
                        </button>
                    </div>
                {:else}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden flex-1">
                        {#each roles as role}
                            <div
                                class="border-b border-zinc-800/50 px-3 py-2.5 cursor-pointer transition-colors {selectedRole?.roleName === role.roleName ? 'bg-orange-900/20 border-l-2 border-l-orange-500' : 'hover:bg-zinc-800/50'}"
                                onclick={() => selectRole(role)}
                            >
                                <div class="flex items-center justify-between">
                                    <div class="flex items-center gap-1.5 min-w-0">
                                        <span class="font-mono text-sm text-orange-400 truncate">{role.roleName}</span>
                                        {#if isServiceLinkedRole(role)}
                                            <span class="text-xs bg-blue-900/40 text-blue-400 px-1 rounded flex-shrink-0">SLR</span>
                                        {/if}
                                    </div>
                                    {#if confirmDeleteRole === role.roleName}
                                        <div class="flex items-center gap-1" onclick={(e) => e.stopPropagation()}>
                                            <button onclick={() => handleDeleteRole(role.roleName)} class="px-1.5 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteRole = null} class="px-1.5 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button
                                            onclick={(e) => { e.stopPropagation(); confirmDeleteRole = role.roleName; }}
                                            class="text-red-400 hover:text-red-300 text-xs px-1.5 py-0.5 rounded hover:bg-red-900/30 transition-colors flex-shrink-0"
                                        >
                                            Delete
                                        </button>
                                    {/if}
                                </div>
                                <div class="text-xs text-zinc-500 font-mono mt-0.5 truncate">{role.arn}</div>
                            </div>
                        {/each}
                    </div>
                {/if}
            </div>

            <!-- Role detail panel -->
            {#if selectedRole}
                <div class="flex-1 min-w-0 bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden flex flex-col">
                    <div class="px-4 py-3 border-b border-zinc-800">
                        <span class="font-semibold">{selectedRole.roleName}</span>
                        {#if selectedRole.description}
                            <p class="text-zinc-400 text-xs mt-0.5">{selectedRole.description}</p>
                        {/if}
                    </div>

                    <!-- Role sub-tabs -->
                    <div class="flex border-b border-zinc-800 px-4">
                        {#each [
                            { id: 'details', label: 'Details' },
                            { id: 'trustpolicy', label: 'Trust Policy' },
                            { id: 'policies', label: 'Policies' },
                        ] as sub}
                            <button
                                onclick={() => roleSubTab = sub.id as typeof roleSubTab}
                                class="px-3 py-2 text-sm border-b-2 transition-colors {roleSubTab === sub.id ? 'border-orange-400 text-orange-400' : 'border-transparent text-zinc-500 hover:text-zinc-300'}"
                            >
                                {sub.label}
                            </button>
                        {/each}
                    </div>

                    {#if roleActionError}
                        <div class="mx-4 mt-3 bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm">{roleActionError}</div>
                    {/if}

                    {#if roleDetailLoading}
                        <div class="p-4 text-zinc-500 text-sm">Loading...</div>
                    {:else}
                        <div class="p-4 overflow-y-auto flex-1">
                            {#if roleSubTab === 'details'}
                                <div class="space-y-3">
                                    <div class="bg-zinc-800/50 rounded-lg p-3">
                                        <div class="flex items-center justify-between mb-1">
                                            <span class="text-zinc-400 text-xs uppercase tracking-wide">ARN</span>
                                            <button onclick={() => copyToClipboard(selectedRole!.arn)} class="text-xs text-zinc-500 hover:text-zinc-300">Copy</button>
                                        </div>
                                        <p class="font-mono text-xs text-zinc-200 break-all">{selectedRole.arn}</p>
                                    </div>
                                    <div class="bg-zinc-800/50 rounded-lg p-3">
                                        <span class="text-zinc-400 text-xs uppercase tracking-wide">Role ID</span>
                                        <p class="font-mono text-xs text-zinc-200 mt-1">{selectedRole.roleId}</p>
                                    </div>
                                    {#if selectedRole.createDate}
                                        <div class="bg-zinc-800/50 rounded-lg p-3">
                                            <span class="text-zinc-400 text-xs uppercase tracking-wide">Created</span>
                                            <p class="text-sm text-zinc-200 mt-1">{formatDate(selectedRole.createDate)}</p>
                                        </div>
                                    {/if}
                                    {#if isServiceLinkedRole(selectedRole)}
                                        <div class="bg-blue-900/20 border border-blue-800 rounded-lg p-3">
                                            <p class="text-blue-400 text-sm">This is an AWS service-linked role and cannot be modified directly.</p>
                                        </div>
                                    {/if}
                                </div>

                            {:else if roleSubTab === 'trustpolicy'}
                                <div class="space-y-3">
                                    <div class="flex items-center justify-between">
                                        <h4 class="text-sm font-semibold text-zinc-300">Assume Role Policy Document</h4>
                                        <div class="flex items-center gap-2">
                                            {#if trustPolicySaved}
                                                <span class="text-green-400 text-xs">Saved</span>
                                            {/if}
                                            {#if !editingTrustPolicy}
                                                <button onclick={() => editingTrustPolicy = true} class="px-2 py-1 text-xs bg-zinc-700 hover:bg-zinc-600 rounded transition-colors">Edit</button>
                                            {/if}
                                        </div>
                                    </div>
                                    {#if editingTrustPolicy}
                                        <textarea bind:value={editedTrustPolicy} rows="16"
                                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y"></textarea>
                                        <div class="flex gap-2">
                                            <button onclick={handleSaveTrustPolicy} disabled={savingTrustPolicy}
                                                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm transition-colors">
                                                {savingTrustPolicy ? 'Saving...' : 'Save'}
                                            </button>
                                            <button onclick={() => { editingTrustPolicy = false; editedTrustPolicy = formatJson(selectedRole?.assumeRolePolicyDocument || '{}'); }}
                                                class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors">Cancel</button>
                                        </div>
                                    {:else}
                                        <pre class="bg-zinc-950 rounded-lg p-3 text-xs font-mono text-zinc-300 overflow-x-auto whitespace-pre-wrap break-all">{formatJson(selectedRole?.assumeRolePolicyDocument || '{}')}</pre>
                                    {/if}
                                </div>

                            {:else if roleSubTab === 'policies'}
                                <div class="space-y-4">
                                    <!-- Inline -->
                                    <div>
                                        <h4 class="text-sm font-semibold mb-2 text-zinc-300">Inline Policies</h4>
                                        {#if roleInlinePolicies.length === 0}
                                            <p class="text-zinc-500 text-sm">No inline policies.</p>
                                        {:else}
                                            {#each roleInlinePolicies as pol}
                                                <div class="bg-zinc-800 rounded px-3 py-2 text-sm font-mono text-zinc-300 mb-1">{pol}</div>
                                            {/each}
                                        {/if}
                                    </div>
                                    <!-- Attached -->
                                    <div>
                                        <h4 class="text-sm font-semibold mb-2 text-zinc-300">Attached Managed Policies</h4>
                                        {#if roleAttachedPolicies.length === 0}
                                            <p class="text-zinc-500 text-sm">No attached policies.</p>
                                        {:else}
                                            <div class="space-y-1">
                                                {#each roleAttachedPolicies as pol}
                                                    <div class="bg-zinc-800 rounded px-3 py-2 flex items-center justify-between">
                                                        <div>
                                                            <div class="text-sm font-mono text-zinc-200">{pol.policyName}</div>
                                                            <div class="text-xs text-zinc-500 font-mono">{pol.policyArn}</div>
                                                        </div>
                                                        {#if confirmDetachRolePolicy === pol.policyArn}
                                                            <div class="flex gap-1">
                                                                <button onclick={() => handleDetachRolePolicy(pol.policyArn)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                                                <button onclick={() => confirmDetachRolePolicy = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                                            </div>
                                                        {:else}
                                                            <button onclick={() => confirmDetachRolePolicy = pol.policyArn} class="text-xs text-red-400 hover:text-red-300 px-2 py-0.5 rounded hover:bg-red-900/30 transition-colors">Detach</button>
                                                        {/if}
                                                    </div>
                                                {/each}
                                            </div>
                                        {/if}
                                        <div class="mt-3 flex gap-2">
                                            <select bind:value={attachRolePolicyArn}
                                                class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500">
                                                <option value="">-- Select policy to attach --</option>
                                                {#each policies as pol}
                                                    <option value={pol.arn}>{pol.policyName}</option>
                                                {/each}
                                            </select>
                                            <button onclick={handleAttachRolePolicy} disabled={attachingRolePolicy || !attachRolePolicyArn}
                                                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm transition-colors">
                                                {attachingRolePolicy ? 'Attaching...' : 'Attach'}
                                            </button>
                                        </div>
                                    </div>
                                </div>
                            {/if}
                        </div>
                    {/if}
                </div>
            {:else}
                <div class="flex-1 flex items-center justify-center text-zinc-600">
                    <p>Select a role to view details</p>
                </div>
            {/if}
        </div>
    {/if}

    <!-- ============ POLICIES TAB ============ -->
    {#if activeTab === 'policies'}
        {#if showCreatePolicy}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create Managed Policy</h3>
                {#if createPolicyError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createPolicyError}</div>
                {/if}
                <div class="grid grid-cols-2 gap-3 mb-3">
                    <div>
                        <label class="block text-xs text-zinc-400 mb-1">Policy Name</label>
                        <input type="text" bind:value={newPolicyName} placeholder="MyPolicy"
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" />
                    </div>
                    <div>
                        <label class="block text-xs text-zinc-400 mb-1">Description (optional)</label>
                        <input type="text" bind:value={newPolicyDescription} placeholder="Policy description"
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" />
                    </div>
                </div>
                <div class="mb-3">
                    <label class="block text-xs text-zinc-400 mb-1">Policy Document (JSON)</label>
                    <textarea bind:value={newPolicyDocument} rows="10"
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y"></textarea>
                </div>
                <div class="flex gap-2">
                    <button onclick={handleCreatePolicy} disabled={creatingPolicy || !newPolicyName.trim()}
                        class="px-4 py-2 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm font-medium transition-colors">
                        {creatingPolicy ? 'Creating...' : 'Create Policy'}
                    </button>
                    <button onclick={() => { showCreatePolicy = false; createPolicyError = null; }}
                        class="px-3 py-2 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors">Cancel</button>
                </div>
            </div>
        {/if}

        <div class="flex gap-4 flex-1 min-h-0">
            <!-- Policy list -->
            <div class="w-80 flex-shrink-0 flex flex-col">
                {#if policiesLoading}
                    <div class="text-zinc-500 text-sm">Loading policies...</div>
                {:else if policiesError}
                    <div class="bg-red-900/20 border border-red-800 rounded-lg p-3 text-red-400 text-sm">{policiesError}</div>
                {:else if policies.length === 0}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-6 text-center">
                        <p class="text-zinc-500 text-sm">No local IAM policies yet.</p>
                        <button onclick={() => showCreatePolicy = true} class="mt-2 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                            Create Policy
                        </button>
                    </div>
                {:else}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden flex-1">
                        {#each policies as policy}
                            <div
                                class="border-b border-zinc-800/50 px-3 py-2.5 cursor-pointer transition-colors {selectedPolicy?.arn === policy.arn ? 'bg-orange-900/20 border-l-2 border-l-orange-500' : 'hover:bg-zinc-800/50'}"
                                onclick={() => selectPolicy(policy)}
                            >
                                <div class="flex items-center justify-between">
                                    <span class="font-mono text-sm text-orange-400 truncate">{policy.policyName}</span>
                                    {#if confirmDeletePolicy === policy.arn}
                                        <div class="flex items-center gap-1" onclick={(e) => e.stopPropagation()}>
                                            <button onclick={() => handleDeletePolicy(policy.arn)} class="px-1.5 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeletePolicy = null} class="px-1.5 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button
                                            onclick={(e) => { e.stopPropagation(); confirmDeletePolicy = policy.arn; }}
                                            class="text-red-400 hover:text-red-300 text-xs px-1.5 py-0.5 rounded hover:bg-red-900/30 transition-colors flex-shrink-0"
                                        >
                                            Delete
                                        </button>
                                    {/if}
                                </div>
                                <div class="text-xs text-zinc-500 mt-0.5 flex gap-3">
                                    <span>{policy.attachmentCount} attached</span>
                                </div>
                            </div>
                        {/each}
                    </div>
                {/if}
            </div>

            <!-- Policy detail panel -->
            {#if selectedPolicy}
                <div class="flex-1 min-w-0 bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden flex flex-col">
                    <div class="px-4 py-3 border-b border-zinc-800">
                        <span class="font-semibold">{selectedPolicy.policyName}</span>
                        {#if selectedPolicy.description}
                            <p class="text-zinc-400 text-xs mt-0.5">{selectedPolicy.description}</p>
                        {/if}
                    </div>

                    <!-- Policy sub-tabs -->
                    <div class="flex border-b border-zinc-800 px-4">
                        {#each [
                            { id: 'details', label: 'Details' },
                            { id: 'document', label: 'Policy Document' },
                            { id: 'versions', label: 'Versions' },
                        ] as sub}
                            <button
                                onclick={() => policySubTab = sub.id as typeof policySubTab}
                                class="px-3 py-2 text-sm border-b-2 transition-colors {policySubTab === sub.id ? 'border-orange-400 text-orange-400' : 'border-transparent text-zinc-500 hover:text-zinc-300'}"
                            >
                                {sub.label}
                            </button>
                        {/each}
                    </div>

                    {#if policyActionError}
                        <div class="mx-4 mt-3 bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm">{policyActionError}</div>
                    {/if}

                    {#if policyDetailLoading}
                        <div class="p-4 text-zinc-500 text-sm">Loading...</div>
                    {:else}
                        <div class="p-4 overflow-y-auto flex-1">
                            {#if policySubTab === 'details'}
                                <div class="space-y-3">
                                    <div class="bg-zinc-800/50 rounded-lg p-3">
                                        <div class="flex items-center justify-between mb-1">
                                            <span class="text-zinc-400 text-xs uppercase tracking-wide">ARN</span>
                                            <button onclick={() => copyToClipboard(selectedPolicy!.arn)} class="text-xs text-zinc-500 hover:text-zinc-300">Copy</button>
                                        </div>
                                        <p class="font-mono text-xs text-zinc-200 break-all">{selectedPolicy.arn}</p>
                                    </div>
                                    <div class="bg-zinc-800/50 rounded-lg p-3 grid grid-cols-2 gap-3">
                                        <div>
                                            <span class="text-zinc-400 text-xs uppercase tracking-wide">Attachments</span>
                                            <p class="text-sm text-zinc-200 mt-1">{selectedPolicy.attachmentCount}</p>
                                        </div>
                                        <div>
                                            <span class="text-zinc-400 text-xs uppercase tracking-wide">Default Version</span>
                                            <p class="text-sm text-zinc-200 mt-1">{selectedPolicy.defaultVersionId ?? '—'}</p>
                                        </div>
                                    </div>
                                    {#if selectedPolicy.createDate}
                                        <div class="bg-zinc-800/50 rounded-lg p-3">
                                            <span class="text-zinc-400 text-xs uppercase tracking-wide">Created</span>
                                            <p class="text-sm text-zinc-200 mt-1">{formatDate(selectedPolicy.createDate)}</p>
                                        </div>
                                    {/if}
                                </div>

                            {:else if policySubTab === 'document'}
                                <div>
                                    <div class="flex items-center justify-between mb-2">
                                        <h4 class="text-sm font-semibold text-zinc-300">Default Version Document</h4>
                                        <button onclick={() => copyToClipboard(policyDocument)} class="text-xs text-zinc-500 hover:text-zinc-300">Copy</button>
                                    </div>
                                    <pre class="bg-zinc-950 rounded-lg p-3 text-xs font-mono text-zinc-300 overflow-x-auto whitespace-pre-wrap break-all">{policyDocument || 'No document available'}</pre>
                                </div>

                            {:else if policySubTab === 'versions'}
                                <div class="space-y-4">
                                    <!-- Create version form -->
                                    {#if showCreateVersion}
                                        <div class="bg-zinc-800 rounded-lg p-3">
                                            <h4 class="text-sm font-semibold mb-2">Create New Version</h4>
                                            <textarea bind:value={newVersionDocument} rows="8" placeholder="Paste policy JSON..."
                                                class="w-full bg-zinc-900 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y mb-2"></textarea>
                                            <div class="flex items-center gap-2 mb-2">
                                                <label class="flex items-center gap-1.5 text-sm">
                                                    <input type="checkbox" bind:checked={newVersionSetDefault} class="accent-orange-500" />
                                                    Set as default version
                                                </label>
                                            </div>
                                            <div class="flex gap-2">
                                                <button onclick={handleCreateVersion} disabled={creatingVersion || !newVersionDocument.trim()}
                                                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm transition-colors">
                                                    {creatingVersion ? 'Creating...' : 'Create'}
                                                </button>
                                                <button onclick={() => { showCreateVersion = false; newVersionDocument = ''; }}
                                                    class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors">Cancel</button>
                                            </div>
                                        </div>
                                    {:else}
                                        <button onclick={() => { showCreateVersion = true; newVersionDocument = policyDocument; }}
                                            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors">
                                            Create New Version
                                        </button>
                                    {/if}

                                    <!-- Versions list -->
                                    {#if policyVersions.length === 0}
                                        <p class="text-zinc-500 text-sm">No versions found.</p>
                                    {:else}
                                        <div class="space-y-2">
                                            {#each policyVersions as ver}
                                                <div class="bg-zinc-800 rounded-lg p-3">
                                                    <div class="flex items-center justify-between mb-1">
                                                        <div class="flex items-center gap-2">
                                                            <button
                                                                onclick={() => loadPolicyVersion(ver.versionId)}
                                                                class="font-mono text-sm text-zinc-200 hover:text-orange-400 transition-colors"
                                                            >{ver.versionId}</button>
                                                            {#if ver.isDefaultVersion}
                                                                <span class="text-xs bg-green-900/40 text-green-400 px-1.5 py-0.5 rounded">Default</span>
                                                            {/if}
                                                            <span class="text-xs text-zinc-500">{formatDate(ver.createDate)}</span>
                                                        </div>
                                                        <div class="flex items-center gap-1">
                                                            {#if !ver.isDefaultVersion}
                                                                {#if confirmDeleteVersion === ver.versionId}
                                                                    <button onclick={() => handleDeleteVersion(ver.versionId)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                                                    <button onclick={() => confirmDeleteVersion = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                                                {:else}
                                                                    <button onclick={() => confirmDeleteVersion = ver.versionId} class="text-xs text-red-400 hover:text-red-300 px-2 py-0.5 rounded hover:bg-red-900/30 transition-colors">Delete</button>
                                                                {/if}
                                                            {/if}
                                                        </div>
                                                    </div>
                                                    {#if selectedPolicyVersion === ver.versionId && policyVersionDocument}
                                                        <pre class="bg-zinc-950 rounded p-2 text-xs font-mono text-zinc-300 overflow-x-auto whitespace-pre-wrap break-all mt-2">{policyVersionDocument}</pre>
                                                    {/if}
                                                </div>
                                            {/each}
                                        </div>
                                    {/if}
                                </div>
                            {/if}
                        </div>
                    {/if}
                </div>
            {:else}
                <div class="flex-1 flex items-center justify-center text-zinc-600">
                    <p>Select a policy to view details</p>
                </div>
            {/if}
        </div>
    {/if}

    <!-- ============ GROUPS TAB ============ -->
    {#if activeTab === 'groups'}
        {#if showCreateGroup}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create IAM Group</h3>
                {#if createGroupError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createGroupError}</div>
                {/if}
                <div class="flex gap-2">
                    <input type="text" bind:value={newGroupName} placeholder="group-name"
                        onkeydown={(e) => e.key === 'Enter' && handleCreateGroup()}
                        class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500" />
                    <button onclick={handleCreateGroup} disabled={creatingGroup || !newGroupName.trim()}
                        class="px-4 py-2 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm font-medium transition-colors">
                        {creatingGroup ? 'Creating...' : 'Create'}
                    </button>
                    <button onclick={() => { showCreateGroup = false; createGroupError = null; }}
                        class="px-3 py-2 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors">Cancel</button>
                </div>
            </div>
        {/if}

        <div class="flex gap-4 flex-1 min-h-0">
            <!-- Group list -->
            <div class="w-72 flex-shrink-0 flex flex-col">
                {#if groupsLoading}
                    <div class="text-zinc-500 text-sm">Loading groups...</div>
                {:else if groupsError}
                    <div class="bg-red-900/20 border border-red-800 rounded-lg p-3 text-red-400 text-sm">{groupsError}</div>
                {:else if groups.length === 0}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-6 text-center">
                        <p class="text-zinc-500 text-sm">No IAM groups yet.</p>
                        <button onclick={() => showCreateGroup = true} class="mt-2 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                            Create Group
                        </button>
                    </div>
                {:else}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden flex-1">
                        {#each groups as group}
                            <div
                                class="border-b border-zinc-800/50 px-3 py-2.5 cursor-pointer transition-colors {selectedGroup?.groupName === group.groupName ? 'bg-orange-900/20 border-l-2 border-l-orange-500' : 'hover:bg-zinc-800/50'}"
                                onclick={() => selectGroup(group)}
                            >
                                <div class="flex items-center justify-between">
                                    <span class="font-mono text-sm text-orange-400">{group.groupName}</span>
                                    {#if confirmDeleteGroup === group.groupName}
                                        <div class="flex items-center gap-1" onclick={(e) => e.stopPropagation()}>
                                            <button onclick={() => handleDeleteGroup(group.groupName)} class="px-1.5 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteGroup = null} class="px-1.5 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button
                                            onclick={(e) => { e.stopPropagation(); confirmDeleteGroup = group.groupName; }}
                                            class="text-red-400 hover:text-red-300 text-xs px-1.5 py-0.5 rounded hover:bg-red-900/30 transition-colors"
                                        >
                                            Delete
                                        </button>
                                    {/if}
                                </div>
                                <div class="text-xs text-zinc-500 font-mono mt-0.5 truncate">{group.arn}</div>
                            </div>
                        {/each}
                    </div>
                {/if}
            </div>

            <!-- Group detail panel -->
            {#if selectedGroup}
                <div class="flex-1 min-w-0 bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden flex flex-col">
                    <div class="px-4 py-3 border-b border-zinc-800">
                        <span class="font-semibold">{selectedGroup.groupName}</span>
                        <p class="font-mono text-xs text-zinc-500 mt-0.5">{selectedGroup.arn}</p>
                    </div>

                    <div class="flex border-b border-zinc-800 px-4">
                        {#each [{ id: 'members', label: 'Members' }, { id: 'policies', label: 'Policies' }] as sub}
                            <button
                                onclick={() => groupSubTab = sub.id as typeof groupSubTab}
                                class="px-3 py-2 text-sm border-b-2 transition-colors {groupSubTab === sub.id ? 'border-orange-400 text-orange-400' : 'border-transparent text-zinc-500 hover:text-zinc-300'}"
                            >
                                {sub.label}
                            </button>
                        {/each}
                    </div>

                    {#if groupActionError}
                        <div class="mx-4 mt-3 bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm">{groupActionError}</div>
                    {/if}

                    {#if groupDetailLoading}
                        <div class="p-4 text-zinc-500 text-sm">Loading...</div>
                    {:else}
                        <div class="p-4 overflow-y-auto flex-1">
                            {#if groupSubTab === 'members'}
                                <div class="space-y-3">
                                    {#if groupMembers.length === 0}
                                        <p class="text-zinc-500 text-sm">No members.</p>
                                    {:else}
                                        <div class="space-y-1">
                                            {#each groupMembers as member}
                                                <div class="bg-zinc-800 rounded px-3 py-2 flex items-center justify-between">
                                                    <span class="font-mono text-sm text-zinc-200">{member.userName}</span>
                                                    {#if confirmRemoveMember === member.userName}
                                                        <div class="flex gap-1">
                                                            <button onclick={() => handleRemoveMember(member.userName)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                                            <button onclick={() => confirmRemoveMember = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                                        </div>
                                                    {:else}
                                                        <button onclick={() => confirmRemoveMember = member.userName} class="text-xs text-red-400 hover:text-red-300 px-2 py-0.5 rounded hover:bg-red-900/30 transition-colors">Remove</button>
                                                    {/if}
                                                </div>
                                            {/each}
                                        </div>
                                    {/if}
                                    <div class="flex gap-2">
                                        <select bind:value={addMemberName}
                                            class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500">
                                            <option value="">-- Add member --</option>
                                            {#each users as user}
                                                <option value={user.userName}>{user.userName}</option>
                                            {/each}
                                        </select>
                                        <button onclick={handleAddMember} disabled={addingMember || !addMemberName}
                                            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm transition-colors">
                                            {addingMember ? 'Adding...' : 'Add'}
                                        </button>
                                    </div>
                                </div>

                            {:else if groupSubTab === 'policies'}
                                <div class="space-y-3">
                                    {#if groupAttachedPolicies.length === 0}
                                        <p class="text-zinc-500 text-sm">No attached policies.</p>
                                    {:else}
                                        <div class="space-y-1">
                                            {#each groupAttachedPolicies as pol}
                                                <div class="bg-zinc-800 rounded px-3 py-2 flex items-center justify-between">
                                                    <div>
                                                        <div class="text-sm font-mono text-zinc-200">{pol.policyName}</div>
                                                        <div class="text-xs text-zinc-500 font-mono">{pol.policyArn}</div>
                                                    </div>
                                                    {#if confirmDetachGroupPolicy === pol.policyArn}
                                                        <div class="flex gap-1">
                                                            <button onclick={() => handleDetachGroupPolicy(pol.policyArn)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                                            <button onclick={() => confirmDetachGroupPolicy = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                                        </div>
                                                    {:else}
                                                        <button onclick={() => confirmDetachGroupPolicy = pol.policyArn} class="text-xs text-red-400 hover:text-red-300 px-2 py-0.5 rounded hover:bg-red-900/30 transition-colors">Detach</button>
                                                    {/if}
                                                </div>
                                            {/each}
                                        </div>
                                    {/if}
                                    <div class="flex gap-2">
                                        <select bind:value={attachGroupPolicyArn}
                                            class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500">
                                            <option value="">-- Select policy to attach --</option>
                                            {#each policies as pol}
                                                <option value={pol.arn}>{pol.policyName}</option>
                                            {/each}
                                        </select>
                                        <button onclick={handleAttachGroupPolicy} disabled={attachingGroupPolicy || !attachGroupPolicyArn}
                                            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm transition-colors">
                                            {attachingGroupPolicy ? 'Attaching...' : 'Attach'}
                                        </button>
                                    </div>
                                </div>
                            {/if}
                        </div>
                    {/if}
                </div>
            {:else}
                <div class="flex-1 flex items-center justify-center text-zinc-600">
                    <p>Select a group to view details</p>
                </div>
            {/if}
        </div>
    {/if}

    <!-- ============ ACCOUNT TAB ============ -->
    {#if activeTab === 'account'}
        {#if accountLoading}
            <div class="text-zinc-500 text-sm">Loading account information...</div>
        {:else if accountError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400 mb-4">{accountError}</div>
        {:else}
            <div class="space-y-4 max-w-4xl">
                <!-- Account Summary -->
                {#if accountSummary}
                    <div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4">
                        <h3 class="font-semibold mb-3">Account Summary</h3>
                        <div class="grid grid-cols-2 sm:grid-cols-4 gap-3">
                            {#each [
                                { label: 'Users', value: accountSummary.users, quota: accountSummary.usersQuota },
                                { label: 'Roles', value: accountSummary.roles, quota: accountSummary.rolesQuota },
                                { label: 'Groups', value: accountSummary.groups, quota: accountSummary.groupsQuota },
                                { label: 'Policies', value: accountSummary.policies, quota: accountSummary.policiesQuota },
                            ] as stat}
                                <div class="bg-zinc-800/50 rounded-lg p-3 text-center">
                                    <div class="text-2xl font-bold text-orange-400">{stat.value}</div>
                                    <div class="text-xs text-zinc-400 mt-0.5">{stat.label}</div>
                                    <div class="text-xs text-zinc-600">/ {stat.quota} max</div>
                                </div>
                            {/each}
                        </div>
                        <div class="mt-3 grid grid-cols-2 gap-3">
                            <div class="bg-zinc-800/50 rounded-lg p-3">
                                <div class="text-zinc-400 text-xs uppercase tracking-wide">Account Access Keys Present</div>
                                <div class="text-lg font-semibold mt-1">{accountSummary.accountAccessKeysPresent}</div>
                            </div>
                            <div class="bg-zinc-800/50 rounded-lg p-3">
                                <div class="text-zinc-400 text-xs uppercase tracking-wide">Access Keys per User Quota</div>
                                <div class="text-lg font-semibold mt-1">{accountSummary.accessKeysPerUserQuota}</div>
                            </div>
                        </div>
                    </div>
                {/if}

                <!-- Account Aliases -->
                <div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4">
                    <h3 class="font-semibold mb-3">Account Aliases</h3>
                    {#if accountAliases.length === 0}
                        <p class="text-zinc-500 text-sm mb-3">No account aliases configured.</p>
                    {:else}
                        <div class="space-y-1 mb-3">
                            {#each accountAliases as alias}
                                <div class="flex items-center justify-between bg-zinc-800 rounded px-3 py-2">
                                    <span class="font-mono text-sm text-zinc-200">{alias}</span>
                                    {#if confirmDeleteAlias === alias}
                                        <div class="flex gap-1">
                                            <button onclick={() => handleDeleteAlias(alias)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteAlias = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button onclick={() => confirmDeleteAlias = alias} class="text-xs text-red-400 hover:text-red-300 px-2 py-0.5 rounded hover:bg-red-900/30 transition-colors">Delete</button>
                                    {/if}
                                </div>
                            {/each}
                        </div>
                    {/if}
                    <div class="flex gap-2">
                        <input type="text" bind:value={newAlias} placeholder="my-account-alias"
                            onkeydown={(e) => e.key === 'Enter' && handleCreateAlias()}
                            class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500" />
                        <button onclick={handleCreateAlias} disabled={creatingAlias || !newAlias.trim()}
                            class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm transition-colors">
                            {creatingAlias ? 'Creating...' : 'Create Alias'}
                        </button>
                    </div>
                </div>

                <!-- Password Policy -->
                <div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4">
                    <div class="flex items-center justify-between mb-3">
                        <h3 class="font-semibold">Password Policy</h3>
                        <div class="flex items-center gap-2">
                            {#if passwordPolicySaved}
                                <span class="text-green-400 text-xs">Saved</span>
                            {/if}
                            {#if !editingPasswordPolicy && passwordPolicy}
                                <button onclick={() => { editingPasswordPolicy = true; editedPasswordPolicy = { ...passwordPolicy! }; }}
                                    class="px-2 py-1 text-xs bg-zinc-700 hover:bg-zinc-600 rounded transition-colors">Edit</button>
                            {/if}
                        </div>
                    </div>

                    {#if !passwordPolicy}
                        <p class="text-zinc-500 text-sm">No password policy configured.</p>
                    {:else if editingPasswordPolicy && editedPasswordPolicy}
                        <div class="space-y-3">
                            <div class="grid grid-cols-2 gap-4">
                                <div>
                                    <label class="block text-xs text-zinc-400 mb-1">Minimum Password Length</label>
                                    <input type="number" bind:value={editedPasswordPolicy.minimumPasswordLength} min="6" max="128"
                                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500" />
                                </div>
                                <div>
                                    <label class="block text-xs text-zinc-400 mb-1">Max Password Age (days, 0=never)</label>
                                    <input type="number" bind:value={editedPasswordPolicy.maxPasswordAge} min="0" max="1095"
                                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500" />
                                </div>
                                <div>
                                    <label class="block text-xs text-zinc-400 mb-1">Password Reuse Prevention (0=disabled)</label>
                                    <input type="number" bind:value={editedPasswordPolicy.passwordReusePrevention} min="0" max="24"
                                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm focus:outline-none focus:border-orange-500" />
                                </div>
                            </div>
                            <div class="space-y-2">
                                {#each [
                                    { key: 'requireUppercaseCharacters' as const, label: 'Require uppercase characters' },
                                    { key: 'requireLowercaseCharacters' as const, label: 'Require lowercase characters' },
                                    { key: 'requireNumbers' as const, label: 'Require numbers' },
                                    { key: 'requireSymbols' as const, label: 'Require symbols' },
                                    { key: 'allowUsersToChangePassword' as const, label: 'Allow users to change password' },
                                    { key: 'hardExpiry' as const, label: 'Hard expiry (prevent login after password expiry)' },
                                ] as setting}
                                    <label class="flex items-center gap-3 cursor-pointer">
                                        <button
                                            type="button"
                                            onclick={() => { if (editedPasswordPolicy) editedPasswordPolicy[setting.key] = !editedPasswordPolicy[setting.key]; }}
                                            class="relative w-9 h-5 rounded-full transition-colors {editedPasswordPolicy[setting.key] ? 'bg-orange-600' : 'bg-zinc-600'}"
                                        >
                                            <span class="absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform {editedPasswordPolicy[setting.key] ? 'translate-x-4' : ''}"></span>
                                        </button>
                                        <span class="text-sm text-zinc-300">{setting.label}</span>
                                    </label>
                                {/each}
                            </div>
                            <div class="flex gap-2 pt-2">
                                <button onclick={handleSavePasswordPolicy} disabled={savingPasswordPolicy}
                                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded text-sm font-medium transition-colors">
                                    {savingPasswordPolicy ? 'Saving...' : 'Save Policy'}
                                </button>
                                <button onclick={() => { editingPasswordPolicy = false; editedPasswordPolicy = passwordPolicy ? { ...passwordPolicy } : null; }}
                                    class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors">Cancel</button>
                            </div>
                        </div>
                    {:else}
                        <div class="grid grid-cols-2 gap-3">
                            <div class="bg-zinc-800/50 rounded-lg p-3">
                                <div class="text-zinc-400 text-xs uppercase tracking-wide mb-1">Min Length</div>
                                <div class="text-lg font-semibold">{passwordPolicy.minimumPasswordLength}</div>
                            </div>
                            <div class="bg-zinc-800/50 rounded-lg p-3">
                                <div class="text-zinc-400 text-xs uppercase tracking-wide mb-1">Max Age</div>
                                <div class="text-lg font-semibold">{passwordPolicy.maxPasswordAge || 'Never'}</div>
                            </div>
                        </div>
                        <div class="mt-3 space-y-1.5">
                            {#each [
                                { label: 'Require uppercase', value: passwordPolicy.requireUppercaseCharacters },
                                { label: 'Require lowercase', value: passwordPolicy.requireLowercaseCharacters },
                                { label: 'Require numbers', value: passwordPolicy.requireNumbers },
                                { label: 'Require symbols', value: passwordPolicy.requireSymbols },
                                { label: 'Allow users to change password', value: passwordPolicy.allowUsersToChangePassword },
                                { label: 'Hard expiry', value: passwordPolicy.hardExpiry },
                            ] as item}
                                <div class="flex items-center gap-2 text-sm">
                                    <span class="w-4 h-4 rounded-full flex items-center justify-center text-xs {item.value ? 'bg-green-900/40 text-green-400' : 'bg-zinc-700 text-zinc-500'}">{item.value ? '✓' : '×'}</span>
                                    <span class="text-zinc-300">{item.label}</span>
                                </div>
                            {/each}
                        </div>
                    {/if}
                </div>
            </div>
        {/if}
    {/if}
</div>
