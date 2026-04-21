<script lang="ts">
    import { onMount } from 'svelte';

    interface ScheduleSummary {
        Arn: string;
        Name: string;
        GroupName: string;
        ScheduleExpression: string;
        State: string;
        CreationDate: number;
    }

    interface ScheduleGroupSummary {
        Arn: string;
        Name: string;
        State: string;
        CreationDate: number;
    }

    let activeTab = $state<'schedules' | 'groups'>('schedules');
    let schedules = $state<ScheduleSummary[]>([]);
    let groups = $state<ScheduleGroupSummary[]>([]);
    let schedulesLoading = $state(false);
    let groupsLoading = $state(false);
    let error = $state<string | null>(null);

    let showCreateSchedule = $state(false);
    let newScheduleName = $state('');
    let newExpression = $state('rate(1 hour)');
    let newTargetArn = $state('');
    let newTargetRoleArn = $state('');
    let newScheduleGroup = $state('default');
    let creatingSchedule = $state(false);
    let createScheduleError = $state<string | null>(null);

    let showCreateGroup = $state(false);
    let newGroupName = $state('');
    let creatingGroup = $state(false);

    async function schedulerFetch(method: string, path: string, body?: Record<string, unknown>) {
        const opts: RequestInit = {
            method,
            headers: { 'Content-Type': 'application/json', Authorization: 'AWS4-HMAC-SHA256 Credential=local/20240101/us-east-1/scheduler/aws4_request' },
        };
        if (body) opts.body = JSON.stringify(body);
        const res = await fetch(`http://localhost:4566${path}`, opts);
        const data = await res.json();
        if (!res.ok) throw new Error(data.message ?? res.statusText);
        return data;
    }

    async function loadSchedules() {
        schedulesLoading = true;
        error = null;
        try {
            const data = await schedulerFetch('GET', '/schedules');
            schedules = data.Schedules ?? [];
        } catch (e: any) {
            error = e.message;
        } finally {
            schedulesLoading = false;
        }
    }

    async function loadGroups() {
        groupsLoading = true;
        error = null;
        try {
            const data = await schedulerFetch('GET', '/schedule-groups');
            groups = data.ScheduleGroups ?? [];
        } catch (e: any) {
            error = e.message;
        } finally {
            groupsLoading = false;
        }
    }

    async function createSchedule() {
        if (!newScheduleName.trim() || !newTargetArn.trim()) return;
        creatingSchedule = true;
        createScheduleError = null;
        try {
            await schedulerFetch('POST', `/schedules/${encodeURIComponent(newScheduleName.trim())}`, {
                ScheduleExpression: newExpression,
                Target: { Arn: newTargetArn.trim(), RoleArn: newTargetRoleArn.trim() || undefined },
                GroupName: newScheduleGroup || 'default',
                FlexibleTimeWindow: { Mode: 'OFF' },
                State: 'ENABLED',
            });
            newScheduleName = '';
            newTargetArn = '';
            newTargetRoleArn = '';
            showCreateSchedule = false;
            await loadSchedules();
        } catch (e: any) {
            createScheduleError = e.message;
        } finally {
            creatingSchedule = false;
        }
    }

    async function deleteSchedule(name: string, group: string) {
        if (!confirm(`Delete schedule "${name}"?`)) return;
        await schedulerFetch('DELETE', `/schedules/${encodeURIComponent(name)}?groupName=${encodeURIComponent(group)}`);
        await loadSchedules();
    }

    async function createGroup() {
        if (!newGroupName.trim()) return;
        creatingGroup = true;
        try {
            await schedulerFetch('POST', `/schedule-groups/${encodeURIComponent(newGroupName.trim())}`, {});
            newGroupName = '';
            showCreateGroup = false;
            await loadGroups();
        } catch (e: any) {
            alert(e.message);
        } finally {
            creatingGroup = false;
        }
    }

    async function deleteGroup(name: string) {
        if (name === 'default') { alert('Cannot delete the default group.'); return; }
        if (!confirm(`Delete group "${name}"?`)) return;
        await schedulerFetch('DELETE', `/schedule-groups/${encodeURIComponent(name)}`);
        await loadGroups();
    }

    function switchTab(tab: 'schedules' | 'groups') {
        activeTab = tab;
        if (tab === 'schedules') loadSchedules();
        else loadGroups();
    }

    onMount(() => loadSchedules());
</script>

<div class="p-6 max-w-5xl mx-auto">
    <div class="mb-6">
        <h1 class="text-2xl font-bold text-zinc-100">EventBridge Scheduler</h1>
        <p class="text-zinc-400 text-sm mt-1">Manage schedules and schedule groups</p>
    </div>

    <!-- Tabs -->
    <div class="flex gap-1 mb-6 border-b border-zinc-800">
        {#each [['schedules', 'Schedules'], ['groups', 'Schedule Groups']] as [id, label]}
            <button
                onclick={() => switchTab(id as 'schedules' | 'groups')}
                class="px-4 py-2 text-sm font-medium transition-colors {activeTab === id
                    ? 'text-orange-400 border-b-2 border-orange-400'
                    : 'text-zinc-400 hover:text-zinc-200'}"
            >
                {label}
            </button>
        {/each}
    </div>

    {#if activeTab === 'schedules'}
        <div class="flex justify-between items-center mb-4">
            <h2 class="text-sm font-semibold text-zinc-300">Schedules</h2>
            <button
                onclick={() => (showCreateSchedule = !showCreateSchedule)}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 text-white rounded text-xs font-medium"
            >
                Create Schedule
            </button>
        </div>

        {#if showCreateSchedule}
            <div class="mb-4 p-4 bg-zinc-900 border border-zinc-700 rounded-lg space-y-3">
                <div class="grid grid-cols-2 gap-3">
                    <div>
                        <label class="block text-xs text-zinc-400 mb-1">Name</label>
                        <input type="text" bind:value={newScheduleName} placeholder="my-schedule" class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm text-zinc-100 focus:outline-none focus:border-orange-500" />
                    </div>
                    <div>
                        <label class="block text-xs text-zinc-400 mb-1">Group</label>
                        <input type="text" bind:value={newScheduleGroup} placeholder="default" class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm text-zinc-100 focus:outline-none focus:border-orange-500" />
                    </div>
                </div>
                <div>
                    <label class="block text-xs text-zinc-400 mb-1">Schedule Expression</label>
                    <input type="text" bind:value={newExpression} placeholder="rate(1 hour) or cron(0 12 * * ? *)" class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm text-zinc-100 focus:outline-none focus:border-orange-500" />
                </div>
                <div>
                    <label class="block text-xs text-zinc-400 mb-1">Target ARN</label>
                    <input type="text" bind:value={newTargetArn} placeholder="arn:aws:lambda:us-east-1:000000000000:function:my-fn" class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm text-zinc-100 focus:outline-none focus:border-orange-500" />
                </div>
                <div>
                    <label class="block text-xs text-zinc-400 mb-1">Target Role ARN (optional)</label>
                    <input type="text" bind:value={newTargetRoleArn} placeholder="arn:aws:iam::000000000000:role/scheduler-role" class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm text-zinc-100 focus:outline-none focus:border-orange-500" />
                </div>
                {#if createScheduleError}
                    <p class="text-red-400 text-xs">{createScheduleError}</p>
                {/if}
                <div class="flex gap-2">
                    <button onclick={createSchedule} disabled={creatingSchedule || !newScheduleName.trim() || !newTargetArn.trim()} class="px-4 py-2 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 text-white rounded text-sm">
                        {creatingSchedule ? 'Creating...' : 'Create'}
                    </button>
                    <button onclick={() => (showCreateSchedule = false)} class="px-4 py-2 bg-zinc-700 hover:bg-zinc-600 text-zinc-200 rounded text-sm">Cancel</button>
                </div>
            </div>
        {/if}

        {#if schedulesLoading}
            <p class="text-zinc-400 text-sm">Loading...</p>
        {:else if error}
            <p class="text-red-400 text-sm">{error}</p>
        {:else if schedules.length === 0}
            <div class="text-center py-12 text-zinc-500 text-sm">No schedules yet.</div>
        {:else}
            <div class="space-y-2">
                {#each schedules as s}
                    <div class="flex items-center justify-between p-4 bg-zinc-900 border border-zinc-800 rounded-lg">
                        <div class="min-w-0 flex-1">
                            <div class="flex items-center gap-2">
                                <p class="text-sm font-medium text-zinc-100">{s.Name}</p>
                                <span class="text-xs text-zinc-500">({s.GroupName})</span>
                            </div>
                            <p class="text-xs text-zinc-400 mt-0.5 font-mono">{s.ScheduleExpression}</p>
                        </div>
                        <div class="flex items-center gap-3 ml-4">
                            <span class="px-2 py-0.5 rounded text-xs {s.State === 'ENABLED' ? 'bg-green-900 text-green-300' : 'bg-zinc-700 text-zinc-400'}">
                                {s.State}
                            </span>
                            <button onclick={() => deleteSchedule(s.Name, s.GroupName)} class="px-3 py-1.5 text-xs bg-red-900 hover:bg-red-800 text-red-200 rounded">Delete</button>
                        </div>
                    </div>
                {/each}
            </div>
        {/if}

    {:else}
        <div class="flex justify-between items-center mb-4">
            <h2 class="text-sm font-semibold text-zinc-300">Schedule Groups</h2>
            <button onclick={() => (showCreateGroup = !showCreateGroup)} class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 text-white rounded text-xs font-medium">
                Create Group
            </button>
        </div>

        {#if showCreateGroup}
            <div class="mb-4 p-4 bg-zinc-900 border border-zinc-700 rounded-lg space-y-3">
                <div>
                    <label class="block text-xs text-zinc-400 mb-1">Name</label>
                    <input type="text" bind:value={newGroupName} placeholder="my-group" class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm text-zinc-100 focus:outline-none focus:border-orange-500" />
                </div>
                <div class="flex gap-2">
                    <button onclick={createGroup} disabled={creatingGroup || !newGroupName.trim()} class="px-4 py-2 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 text-white rounded text-sm">
                        {creatingGroup ? 'Creating...' : 'Create'}
                    </button>
                    <button onclick={() => (showCreateGroup = false)} class="px-4 py-2 bg-zinc-700 hover:bg-zinc-600 text-zinc-200 rounded text-sm">Cancel</button>
                </div>
            </div>
        {/if}

        {#if groupsLoading}
            <p class="text-zinc-400 text-sm">Loading...</p>
        {:else if error}
            <p class="text-red-400 text-sm">{error}</p>
        {:else if groups.length === 0}
            <div class="text-center py-12 text-zinc-500 text-sm">No schedule groups.</div>
        {:else}
            <div class="space-y-2">
                {#each groups as g}
                    <div class="flex items-center justify-between p-4 bg-zinc-900 border border-zinc-800 rounded-lg">
                        <div>
                            <p class="text-sm font-medium text-zinc-100">{g.Name}</p>
                            <p class="text-xs text-zinc-500 font-mono mt-0.5">{g.Arn}</p>
                        </div>
                        <div class="flex items-center gap-3">
                            <span class="px-2 py-0.5 rounded text-xs bg-green-900 text-green-300">{g.State}</span>
                            {#if g.Name !== 'default'}
                                <button onclick={() => deleteGroup(g.Name)} class="px-3 py-1.5 text-xs bg-red-900 hover:bg-red-800 text-red-200 rounded">Delete</button>
                            {/if}
                        </div>
                    </div>
                {/each}
            </div>
        {/if}
    {/if}
</div>
