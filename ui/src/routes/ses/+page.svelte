<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listEmailIdentities, createEmailIdentity, deleteEmailIdentity,
        listEmailTemplates, createEmailTemplate, deleteEmailTemplate,
        type SesIdentity, type SesTemplate,
    } from '$lib/aws';

    let activeTab = $state<'identities' | 'templates'>('identities');

    // --- Identities ---
    let identities = $state<SesIdentity[]>([]);
    let identitiesLoading = $state(false);
    let identitiesError = $state<string | null>(null);
    let showCreateIdentity = $state(false);
    let newIdentityEmail = $state('');
    let creatingIdentity = $state(false);
    let createIdentityError = $state<string | null>(null);
    let confirmDeleteIdentity = $state<string | null>(null);

    // --- Templates ---
    let templates = $state<SesTemplate[]>([]);
    let templatesLoading = $state(false);
    let templatesError = $state<string | null>(null);
    let showCreateTemplate = $state(false);
    let newTemplateName = $state('');
    let newTemplateSubject = $state('');
    let newTemplateHtml = $state('');
    let creatingTemplate = $state(false);
    let createTemplateError = $state<string | null>(null);
    let confirmDeleteTemplate = $state<string | null>(null);

    async function loadIdentities() {
        identitiesLoading = true;
        identitiesError = null;
        try {
            const data = await listEmailIdentities();
            identities = data.identities;
        } catch (e) {
            identitiesError = e instanceof Error ? e.message : 'Failed to load identities';
        } finally {
            identitiesLoading = false;
        }
    }

    async function handleCreateIdentity() {
        if (!newIdentityEmail.trim()) return;
        creatingIdentity = true;
        createIdentityError = null;
        try {
            await createEmailIdentity(newIdentityEmail.trim());
            newIdentityEmail = '';
            showCreateIdentity = false;
            await loadIdentities();
        } catch (e) {
            createIdentityError = e instanceof Error ? e.message : 'Failed to create identity';
        } finally {
            creatingIdentity = false;
        }
    }

    async function handleDeleteIdentity(email: string) {
        try {
            await deleteEmailIdentity(email);
            confirmDeleteIdentity = null;
            await loadIdentities();
        } catch (e) {
            identitiesError = e instanceof Error ? e.message : 'Failed to delete identity';
        }
    }

    async function loadTemplates() {
        templatesLoading = true;
        templatesError = null;
        try {
            const data = await listEmailTemplates();
            templates = data.templates;
        } catch (e) {
            templatesError = e instanceof Error ? e.message : 'Failed to load templates';
        } finally {
            templatesLoading = false;
        }
    }

    async function handleCreateTemplate() {
        if (!newTemplateName.trim() || !newTemplateSubject.trim()) return;
        creatingTemplate = true;
        createTemplateError = null;
        try {
            await createEmailTemplate(newTemplateName.trim(), newTemplateSubject.trim(), newTemplateHtml);
            newTemplateName = '';
            newTemplateSubject = '';
            newTemplateHtml = '';
            showCreateTemplate = false;
            await loadTemplates();
        } catch (e) {
            createTemplateError = e instanceof Error ? e.message : 'Failed to create template';
        } finally {
            creatingTemplate = false;
        }
    }

    async function handleDeleteTemplate(name: string) {
        try {
            await deleteEmailTemplate(name);
            confirmDeleteTemplate = null;
            await loadTemplates();
        } catch (e) {
            templatesError = e instanceof Error ? e.message : 'Failed to delete template';
        }
    }

    function switchTab(tab: 'identities' | 'templates') {
        activeTab = tab;
        if (tab === 'identities' && identities.length === 0 && !identitiesLoading) loadIdentities();
        if (tab === 'templates' && templates.length === 0 && !templatesLoading) loadTemplates();
    }

    function statusColor(status: string): string {
        const s = status.toUpperCase();
        if (s === 'VERIFIED' || s === 'SUCCESS') return 'bg-green-900/30 text-green-400';
        if (s === 'FAILED') return 'bg-red-900/30 text-red-400';
        return 'bg-zinc-800 text-zinc-400';
    }

    function formatDate(iso: string): string {
        try { return new Date(iso).toLocaleString(); } catch { return iso; }
    }

    onMount(() => loadIdentities());
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">SES — Simple Email Service</h1>
            <p class="text-zinc-500 mt-1">Send transactional and marketing emails at scale.</p>
        </div>
        {#if activeTab === 'identities'}
            <button
                onclick={() => { showCreateIdentity = !showCreateIdentity; createIdentityError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Identity
            </button>
        {:else}
            <button
                onclick={() => { showCreateTemplate = !showCreateTemplate; createTemplateError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Template
            </button>
        {/if}
    </div>

    <!-- Tab nav -->
    <div class="flex gap-1 mb-4 border-b border-zinc-800">
        {#each [['identities', 'Identities'], ['templates', 'Templates']] as [tab, label]}
            <button
                onclick={() => switchTab(tab as 'identities' | 'templates')}
                class="px-4 py-2 text-sm font-medium border-b-2 transition-colors {activeTab === tab ? 'border-orange-400 text-orange-400' : 'border-transparent text-zinc-500 hover:text-zinc-300'}"
            >
                {label}
            </button>
        {/each}
    </div>

    <!-- Identities tab -->
    {#if activeTab === 'identities'}
        {#if showCreateIdentity}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create Email Identity</h3>
                {#if createIdentityError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createIdentityError}</div>
                {/if}
                <div class="mb-3">
                    <label for="identity-email" class="block text-xs text-zinc-400 mb-1">Email Address</label>
                    <input
                        id="identity-email"
                        type="email"
                        bind:value={newIdentityEmail}
                        onkeydown={(e) => e.key === 'Enter' && handleCreateIdentity()}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        placeholder="sender@example.com"
                    />
                </div>
                <div class="flex gap-2">
                    <button
                        onclick={handleCreateIdentity}
                        disabled={creatingIdentity || !newIdentityEmail.trim()}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                    >
                        {creatingIdentity ? 'Creating...' : 'Create'}
                    </button>
                    <button
                        onclick={() => { showCreateIdentity = false; createIdentityError = null; newIdentityEmail = ''; }}
                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        {/if}

        {#if identitiesLoading}
            <div class="text-zinc-500">Loading...</div>
        {:else if identitiesError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{identitiesError}</div>
        {:else if identities.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No email identities yet.</p>
                <button onclick={() => showCreateIdentity = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                    Add your first identity
                </button>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Email Identity</th>
                            <th class="px-4 py-3">Type</th>
                            <th class="px-4 py-3">Verification Status</th>
                            <th class="px-4 py-3"></th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each identities as identity}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400 text-sm">{identity.emailIdentity}</td>
                                <td class="px-4 py-3 text-zinc-400 text-xs">{identity.identityType.replace('_', ' ')}</td>
                                <td class="px-4 py-3">
                                    <span class="px-1.5 py-0.5 rounded text-xs font-medium {statusColor(identity.verificationStatus)}">
                                        {identity.verificationStatus}
                                    </span>
                                </td>
                                <td class="px-4 py-3">
                                    {#if confirmDeleteIdentity === identity.emailIdentity}
                                        <div class="flex items-center gap-1">
                                            <button onclick={() => handleDeleteIdentity(identity.emailIdentity)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteIdentity = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button onclick={() => confirmDeleteIdentity = identity.emailIdentity} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                    {/if}
                                </td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}

    <!-- Templates tab -->
    {#if activeTab === 'templates'}
        {#if showCreateTemplate}
            <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
                <h3 class="font-semibold mb-3">Create Email Template</h3>
                {#if createTemplateError}
                    <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createTemplateError}</div>
                {/if}
                <div class="mb-3">
                    <label for="tpl-name" class="block text-xs text-zinc-400 mb-1">Template Name</label>
                    <input
                        id="tpl-name"
                        type="text"
                        bind:value={newTemplateName}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        placeholder="WelcomeEmail"
                    />
                </div>
                <div class="mb-3">
                    <label for="tpl-subject" class="block text-xs text-zinc-400 mb-1">Subject</label>
                    <input
                        id="tpl-subject"
                        type="text"
                        bind:value={newTemplateSubject}
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                        placeholder="Welcome to our service!"
                    />
                </div>
                <div class="mb-3">
                    <label for="tpl-html" class="block text-xs text-zinc-400 mb-1">HTML Body</label>
                    <textarea
                        id="tpl-html"
                        bind:value={newTemplateHtml}
                        rows="6"
                        class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y"
                        placeholder="<h1>Welcome!</h1><p>Thanks for signing up.</p>"
                    ></textarea>
                </div>
                <div class="flex gap-2">
                    <button
                        onclick={handleCreateTemplate}
                        disabled={creatingTemplate || !newTemplateName.trim() || !newTemplateSubject.trim()}
                        class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                    >
                        {creatingTemplate ? 'Creating...' : 'Create'}
                    </button>
                    <button
                        onclick={() => { showCreateTemplate = false; createTemplateError = null; newTemplateName = ''; newTemplateSubject = ''; newTemplateHtml = ''; }}
                        class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        {/if}

        {#if templatesLoading}
            <div class="text-zinc-500">Loading...</div>
        {:else if templatesError}
            <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{templatesError}</div>
        {:else if templates.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No email templates yet.</p>
                <button onclick={() => showCreateTemplate = true} class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium">
                    Create your first template
                </button>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Template Name</th>
                            <th class="px-4 py-3">Created</th>
                            <th class="px-4 py-3"></th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each templates as template}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400 text-sm">{template.templateName}</td>
                                <td class="px-4 py-3 text-zinc-400 text-xs">
                                    {template.createdTimestamp ? formatDate(template.createdTimestamp) : '—'}
                                </td>
                                <td class="px-4 py-3">
                                    {#if confirmDeleteTemplate === template.templateName}
                                        <div class="flex items-center gap-1">
                                            <button onclick={() => handleDeleteTemplate(template.templateName)} class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs">Confirm</button>
                                            <button onclick={() => confirmDeleteTemplate = null} class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs">Cancel</button>
                                        </div>
                                    {:else}
                                        <button onclick={() => confirmDeleteTemplate = template.templateName} class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors">Delete</button>
                                    {/if}
                                </td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}
</div>
