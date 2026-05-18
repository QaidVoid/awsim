<script lang="ts">
	import { page } from '$app/state';
	import { goto, replaceState } from '$app/navigation';
	import { route } from '$lib/url';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		getGroup,
		deleteGroup,
		listAttachedGroupPolicies,
		attachGroupPolicy,
		detachGroupPolicy,
		listGroupPolicies,
		getGroupPolicy,
		putGroupPolicy,
		deleteGroupPolicy,
		listUsers,
		addUserToGroup,
		removeUserFromGroup,
		type IamAttachedPolicy,
		type IamGroup,
		type IamUser
	} from '$lib/api/iam';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import { DetailPage, DetailNavItem } from '$lib/components/service';
	import EntityPoliciesEditor from '$lib/components/iam/entity-policies-editor.svelte';
	import UsersRound from '@lucide/svelte/icons/users-round';
	import FileBadge from '@lucide/svelte/icons/file-badge';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import X from '@lucide/svelte/icons/x';

	const SECTIONS = [
		{ id: 'members', label: 'Members', icon: UsersRound },
		{ id: 'policies', label: 'Policies', icon: FileBadge }
	] as const;

	type SectionId = (typeof SECTIONS)[number]['id'];
	const SECTION_IDS = SECTIONS.map((s) => s.id) as readonly string[];

	let groupName = $derived(decodeURIComponent(page.params.name ?? ''));
	let group = $state<IamGroup | null>(null);
	let loading = $state(true);
	let active = $state<SectionId>(initialSection());
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	function initialSection(): SectionId {
		const tab = page.url.searchParams.get('section');
		return SECTION_IDS.includes(tab ?? '') ? (tab as SectionId) : 'members';
	}

	$effect(() => {
		if (typeof window === 'undefined') return;
		const url = new URL(window.location.href);
		if (url.searchParams.get('section') === active) return;
		url.searchParams.set('section', active);
		replaceState(url.toString(), {});
	});

	async function load() {
		loading = true;
		try {
			const detail = await getGroup(groupName);
			group = detail.group;
			members = detail.users;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load group');
		} finally {
			loading = false;
		}
	}

	onMount(load);

	function handleDelete() {
		deleteOpen = true;
	}

	async function confirmDeleteGroup() {
		deleteBusy = true;
		try {
			await deleteGroup(groupName);
			toast.success(`Deleted ${groupName}`);
			goto(route('/iam'));
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}

	// Members
	let members = $state<IamUser[]>([]);
	let allUsers = $state<IamUser[]>([]);
	let memberPickerName = $state('');
	let addingMember = $state(false);
	let removeMemberName = $state<string | null>(null);
	let removeOpen = $state(false);
	let removeBusy = $state(false);

	const availableUsers = $derived(
		allUsers.filter((u) => !members.some((m) => m.userName === u.userName))
	);

	async function loadAllUsers() {
		if (allUsers.length > 0) return;
		try { allUsers = await listUsers(); } catch { /* empty */ }
	}

	async function addMember() {
		if (!memberPickerName) return;
		addingMember = true;
		try {
			await addUserToGroup(groupName, memberPickerName);
			toast.success(`Added ${memberPickerName}`);
			memberPickerName = '';
			const detail = await getGroup(groupName);
			members = detail.users;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Add failed');
		} finally {
			addingMember = false;
		}
	}

	function removeMember(userName: string) {
		removeMemberName = userName;
		removeOpen = true;
	}

	async function confirmRemoveMember() {
		const userName = removeMemberName;
		if (!userName) return;
		removeBusy = true;
		try {
			await removeUserFromGroup(groupName, userName);
			toast.success(`Removed ${userName}`);
			removeOpen = false;
			removeMemberName = null;
			const detail = await getGroup(groupName);
			members = detail.users;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Remove failed');
		} finally {
			removeBusy = false;
		}
	}

	// Policies
	let attached = $state<IamAttachedPolicy[]>([]);
	let inlineNames = $state<string[]>([]);
	let policiesLoaded = $state(false);

	async function reloadPolicies() {
		const [a, i] = await Promise.all([
			listAttachedGroupPolicies(groupName).catch(() => []),
			listGroupPolicies(groupName).catch(() => [])
		]);
		attached = a;
		inlineNames = i;
	}

	async function loadPolicies() {
		if (policiesLoaded) return;
		await reloadPolicies();
		policiesLoaded = true;
	}

	$effect(() => {
		if (active === 'members') loadAllUsers();
		if (active === 'policies') loadPolicies();
	});
</script>

<DetailPage
	title={groupName}
	subtitle={group?.arn ?? '—'}
	backHref="/iam"
	backLabel="Back to IAM"
	loading={loading}
>
	{#snippet nav()}
		{#each SECTIONS as s (s.id)}
			<DetailNavItem
				icon={s.icon}
				label={s.label}
				active={active === s.id}
				onclick={() => (active = s.id)}
			/>
		{/each}

		<div class="flex-1"></div>

		<button
			type="button"
			class="flex items-center gap-2 rounded px-3 py-2 text-left text-sm text-destructive transition-colors hover:bg-destructive/10"
			onclick={handleDelete}
		>
			<Trash2 class="size-4 shrink-0" />
			Delete group
		</button>
	{/snippet}

	{#if active === 'members'}
				<div class="overflow-y-auto p-6">
					<dl class="mb-6 grid grid-cols-3 gap-x-4 gap-y-2 text-sm">
						<dt class="text-muted-foreground">Group ID</dt>
						<dd class="col-span-2 font-mono text-xs">{group?.groupId ?? '—'}</dd>
						<dt class="text-muted-foreground">ARN</dt>
						<dd class="col-span-2 break-all font-mono text-xs">{group?.arn ?? '—'}</dd>
					</dl>

					<div class="mb-2 flex items-center justify-between">
						<h2 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Members</h2>
						<Badge variant="outline">{members.length}</Badge>
					</div>
					{#if loading}
						<p class="text-xs text-muted-foreground">Loading...</p>
					{:else if members.length === 0}
						<p class="text-xs text-muted-foreground">No users in this group.</p>
					{:else}
						<ul class="space-y-1.5">
							{#each members as m (m.arn)}
								<li class="flex items-center gap-2 rounded border border-border/60 px-3 py-2 text-sm">
									<button
										type="button"
										class="min-w-0 flex-1 text-left"
										onclick={() => goto(route(`/iam/users/${encodeURIComponent(m.userName)}`))}
									>
										<div class="font-medium hover:underline">{m.userName}</div>
										<div class="truncate font-mono text-xs text-muted-foreground">{m.arn}</div>
									</button>
									<Button variant="ghost" size="icon-sm" aria-label="Remove from group" onclick={() => removeMember(m.userName)}>
										<X class="size-3.5" />
									</Button>
								</li>
							{/each}
						</ul>
					{/if}
					<div class="mt-2 flex gap-2">
						<select
							bind:value={memberPickerName}
							class="h-8 flex-1 rounded-md border border-border bg-background px-2 text-xs disabled:opacity-50"
							disabled={availableUsers.length === 0}
						>
							<option value="">
								{availableUsers.length === 0 ? '(no other users)' : 'Select a user to add…'}
							</option>
							{#each availableUsers as u (u.userName)}
								<option value={u.userName}>{u.userName}</option>
							{/each}
						</select>
						<Button size="sm" onclick={addMember} disabled={!memberPickerName || addingMember}>
							<Plus class="size-3.5" />
							<span class="ml-1">Add</span>
						</Button>
					</div>
				</div>
			{:else if active === 'policies'}
				<div class="overflow-y-auto p-6">
					<EntityPoliciesEditor
						{attached}
						{inlineNames}
						onAttach={(arn) => attachGroupPolicy(groupName, arn)}
						onDetach={(arn) => detachGroupPolicy(groupName, arn)}
						onLoadInline={(name) => getGroupPolicy(groupName, name)}
						onPutInline={(name, doc) => putGroupPolicy(groupName, name, doc)}
						onDeleteInline={(name) => deleteGroupPolicy(groupName, name)}
						onMutated={reloadPolicies}
					/>
				</div>
			{/if}
</DetailPage>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete group?"
	description={`Permanently delete "${groupName}". This cannot be undone.`}
	busy={deleteBusy}
	onConfirm={confirmDeleteGroup}
	onClose={() => (deleteOpen = false)}
/>

<ConfirmDialog
	bind:open={removeOpen}
	title="Remove member?"
	description={`Remove ${removeMemberName ?? ''} from "${groupName}".`}
	confirmLabel="Remove"
	busy={removeBusy}
	onConfirm={confirmRemoveMember}
	onClose={() => (removeOpen = false)}
/>
