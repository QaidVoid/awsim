<script lang="ts">
	import { page } from '$app/state';
	import { goto, replaceState } from '$app/navigation';
	import { route } from '$lib/url';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		getUser,
		deleteUser,
		listAttachedUserPolicies,
		attachUserPolicy,
		detachUserPolicy,
		listUserPolicies,
		getUserPolicy,
		putUserPolicy,
		deleteUserPolicy,
		listGroupsForUser,
		removeUserFromGroup,
		type IamUser,
		type IamAttachedPolicy,
		type IamGroup
	} from '$lib/api/iam';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import { DetailPage, DetailNavItem } from '$lib/components/service';
	import EntityPoliciesEditor from '$lib/components/iam/entity-policies-editor.svelte';
	import AccessKeysPanel from '$lib/components/iam/access-keys-panel.svelte';
	import Key from '@lucide/svelte/icons/key';
	import UsersRound from '@lucide/svelte/icons/users-round';
	import FileBadge from '@lucide/svelte/icons/file-badge';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import X from '@lucide/svelte/icons/x';

	const SECTIONS = [
		{ id: 'keys', label: 'Access keys', icon: Key },
		{ id: 'groups', label: 'Groups', icon: UsersRound },
		{ id: 'policies', label: 'Policies', icon: FileBadge }
	] as const;

	type SectionId = (typeof SECTIONS)[number]['id'];
	const SECTION_IDS = SECTIONS.map((s) => s.id) as readonly string[];

	let userName = $derived(decodeURIComponent(page.params.name ?? ''));
	let user = $state<IamUser | null>(null);
	let loading = $state(true);
	let active = $state<SectionId>(initialSection());
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	function initialSection(): SectionId {
		const tab = page.url.searchParams.get('section');
		return SECTION_IDS.includes(tab ?? '') ? (tab as SectionId) : 'keys';
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
			user = await getUser(userName);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load user');
		} finally {
			loading = false;
		}
	}

	onMount(load);

	function handleDelete() {
		deleteOpen = true;
	}

	async function confirmDeleteUser() {
		deleteBusy = true;
		try {
			await deleteUser(userName);
			toast.success(`Deleted ${userName}`);
			goto(route('/iam'));
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}

	// Groups state
	let userGroups = $state<IamGroup[]>([]);
	let groupsLoaded = $state(false);
	let removeGroup = $state<IamGroup | null>(null);
	let removeOpen = $state(false);
	let removeBusy = $state(false);

	async function loadGroups() {
		if (groupsLoaded) return;
		try {
			userGroups = await listGroupsForUser(userName);
		} catch {
			userGroups = [];
		} finally {
			groupsLoaded = true;
		}
	}

	function removeFromGroup(g: IamGroup) {
		removeGroup = g;
		removeOpen = true;
	}

	async function confirmRemoveGroup() {
		const g = removeGroup;
		if (!g) return;
		removeBusy = true;
		try {
			await removeUserFromGroup(g.groupName, userName);
			toast.success(`Removed from ${g.groupName}`);
			removeOpen = false;
			removeGroup = null;
			userGroups = await listGroupsForUser(userName);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed');
		} finally {
			removeBusy = false;
		}
	}

	// Policies state
	let attached = $state<IamAttachedPolicy[]>([]);
	let inlineNames = $state<string[]>([]);

	async function reloadPolicies() {
		const [a, i] = await Promise.all([
			listAttachedUserPolicies(userName).catch(() => []),
			listUserPolicies(userName).catch(() => [])
		]);
		attached = a;
		inlineNames = i;
	}

	let policiesLoaded = $state(false);
	async function loadPolicies() {
		if (policiesLoaded) return;
		await reloadPolicies();
		policiesLoaded = true;
	}

	$effect(() => {
		if (active === 'groups') loadGroups();
		if (active === 'policies') loadPolicies();
	});

	function formatDate(s: string): string {
		try { return new Date(s).toLocaleString(); } catch { return s; }
	}
</script>

<DetailPage
	title={userName}
	subtitle={user?.arn ?? '—'}
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
			Delete user
		</button>
	{/snippet}

	{#if user}
		{#if active === 'keys'}
			<div class="overflow-y-auto p-6">
				<dl class="mb-6 grid grid-cols-3 gap-x-4 gap-y-2 text-sm">
					<dt class="text-muted-foreground">User ID</dt>
					<dd class="col-span-2 font-mono text-xs">{user.userId}</dd>
					<dt class="text-muted-foreground">Created</dt>
					<dd class="col-span-2">{formatDate(user.createDate)}</dd>
					<dt class="text-muted-foreground">ARN</dt>
					<dd class="col-span-2 break-all font-mono text-xs">{user.arn}</dd>
				</dl>
				<AccessKeysPanel {userName} />
			</div>
		{:else if active === 'groups'}
			<div class="overflow-y-auto p-6">
				<div class="mb-2 flex items-center justify-between">
					<h2 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
						Groups
					</h2>
					<Badge variant="outline">{userGroups.length}</Badge>
				</div>
				{#if !groupsLoaded}
					<p class="text-xs text-muted-foreground">Loading...</p>
				{:else if userGroups.length === 0}
					<p class="text-xs text-muted-foreground">Not a member of any group.</p>
				{:else}
					<ul class="space-y-1.5">
						{#each userGroups as g (g.arn)}
							<li class="flex items-center gap-2 rounded border border-border/60 px-3 py-2 text-sm">
								<button
									type="button"
									class="min-w-0 flex-1 text-left"
									onclick={() => goto(route(`/iam/groups/${encodeURIComponent(g.groupName)}`))}
								>
									<div class="font-medium hover:underline">{g.groupName}</div>
									<div class="truncate font-mono text-xs text-muted-foreground">{g.arn}</div>
								</button>
								<Button variant="ghost" size="icon-sm" aria-label="Remove from group" onclick={() => removeFromGroup(g)}>
									<X class="size-3.5" />
								</Button>
							</li>
						{/each}
					</ul>
				{/if}
			</div>
		{:else if active === 'policies'}
			<div class="overflow-y-auto p-6">
				<EntityPoliciesEditor
					{attached}
					{inlineNames}
					onAttach={(arn) => attachUserPolicy(userName, arn)}
					onDetach={(arn) => detachUserPolicy(userName, arn)}
					onLoadInline={(name) => getUserPolicy(userName, name)}
					onPutInline={(name, doc) => putUserPolicy(userName, name, doc)}
					onDeleteInline={(name) => deleteUserPolicy(userName, name)}
					onMutated={reloadPolicies}
				/>
			</div>
		{/if}
	{/if}
</DetailPage>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete user?"
	description={`Permanently delete "${userName}". This cannot be undone.`}
	busy={deleteBusy}
	onConfirm={confirmDeleteUser}
	onClose={() => (deleteOpen = false)}
/>

<ConfirmDialog
	bind:open={removeOpen}
	title="Remove from group?"
	description={`Remove ${userName} from "${removeGroup?.groupName ?? ''}".`}
	confirmLabel="Remove"
	busy={removeBusy}
	onConfirm={confirmRemoveGroup}
	onClose={() => (removeOpen = false)}
/>
