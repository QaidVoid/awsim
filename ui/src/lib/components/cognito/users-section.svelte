<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		listPoolUsers,
		adminEnableUser,
		adminDisableUser,
		adminConfirmSignUp,
		adminResetUserPassword,
		adminDeleteUser,
		type CognitoUserSummary,
		type UserPoolDetail
	} from '$lib/api/cognito';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import UserDetail from './user-detail.svelte';
	import CreateUserDialog from './create-user-dialog.svelte';
	import SetPasswordDialog from './set-password-dialog.svelte';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import ImportUsersDialog from './import-users-dialog.svelte';

	interface Props {
		poolId: string;
		pool: UserPoolDetail | null;
	}

	let { poolId, pool }: Props = $props();

	const PAGE_SIZE_OPTIONS = [25, 50, 100];
	const FILTER_DEBOUNCE_MS = 250;

	let pageSize = $state(50);
	let filter = $state('');
	let users = $state<CognitoUserSummary[]>([]);
	let loading = $state(false);

	/// Cognito's PaginationToken is opaque, so to support a real Prev
	/// button we keep a stack of the tokens used to fetch each page in
	/// history order (oldest first). currentToken is the token we used
	/// to fetch what's on screen right now (undefined for page 1).
	let pageStack = $state<(string | undefined)[]>([]);
	let currentToken = $state<string | undefined>(undefined);
	let nextToken = $state<string | undefined>(undefined);

	let pageIndex = $derived(pageStack.length);
	let filterTimer: ReturnType<typeof setTimeout> | null = null;

	let expandedUser = $state<string | null>(null);
	let createUserOpen = $state(false);
	let importOpen = $state(false);
	let setPwUser = $state<string | null>(null);
	let setPwOpen = $state(false);
	let deleteUser = $state<string | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	function buildFilter(raw: string): string | undefined {
		const t = raw.trim();
		if (!t) return undefined;
		const escaped = t.replace(/"/g, '\\"');
		return `username ^= "${escaped}"`;
	}

	async function fetchPage(token: string | undefined) {
		loading = true;
		try {
			const r = await listPoolUsers(poolId, {
				limit: pageSize,
				paginationToken: token,
				filter: buildFilter(filter)
			});
			users = r.users;
			nextToken = r.nextToken;
			currentToken = token;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load users');
		} finally {
			loading = false;
		}
	}

	async function reset() {
		pageStack = [];
		currentToken = undefined;
		expandedUser = null;
		await fetchPage(undefined);
	}

	async function nextPage() {
		if (!nextToken) return;
		const stackPlus = [...pageStack, currentToken];
		pageStack = stackPlus;
		await fetchPage(nextToken);
	}

	async function prevPage() {
		if (pageStack.length === 0) return;
		const newStack = [...pageStack];
		const t = newStack.pop();
		pageStack = newStack;
		await fetchPage(t);
	}

	$effect(() => {
		// React to filter / pageSize changes only.
		filter;
		pageSize;
		if (filterTimer) clearTimeout(filterTimer);
		filterTimer = setTimeout(() => void reset(), FILTER_DEBOUNCE_MS);
	});

	onMount(() => {
		void fetchPage(undefined);
	});

	async function toggleEnabled(u: CognitoUserSummary) {
		try {
			if (u.enabled) await adminDisableUser(poolId, u.username);
			else await adminEnableUser(poolId, u.username);
			users = users.map((x) =>
				x.username === u.username ? { ...x, enabled: !x.enabled } : x
			);
			toast.success(`${u.username} ${u.enabled ? 'disabled' : 'enabled'}`);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Action failed');
		}
	}

	async function confirmUser(u: CognitoUserSummary) {
		try {
			await adminConfirmSignUp(poolId, u.username);
			toast.success(`${u.username} confirmed`);
			await fetchPage(currentToken);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Confirm failed');
		}
	}

	async function resetPassword(u: CognitoUserSummary) {
		try {
			await adminResetUserPassword(poolId, u.username);
			toast.success(`Reset triggered for ${u.username}`);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Reset failed');
		}
	}

	function openSetPassword(username: string) {
		setPwUser = username;
		setPwOpen = true;
	}

	function openDelete(username: string) {
		deleteUser = username;
		deleteOpen = true;
	}

	async function confirmDeleteUser() {
		if (!deleteUser) return;
		deleteBusy = true;
		try {
			await adminDeleteUser(poolId, deleteUser);
			toast.success(`Deleted ${deleteUser}`);
			deleteOpen = false;
			deleteUser = null;
			await fetchPage(currentToken);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<!-- Sticky toolbar — filter, page-size, refresh, create. Scrolls
	     with the section content so it stays in reach on long lists. -->
	<div
		class="sticky top-0 z-10 flex flex-wrap items-center gap-2 border-b border-border bg-background px-6 py-3"
	>
		<Input
			type="search"
			placeholder="Filter by username prefix..."
			bind:value={filter}
			class="h-8 max-w-xs"
		/>
		<Select
			type="single"
			value={String(pageSize)}
			onValueChange={(v) => (pageSize = Number(v))}
		>
			<SelectTrigger aria-label="Page size" size="sm" class="w-[110px] text-xs">
				{pageSize} / page
			</SelectTrigger>
			<SelectContent>
				{#each PAGE_SIZE_OPTIONS as n (n)}
					<SelectItem value={String(n)} label={`${n} / page`}>{n} / page</SelectItem>
				{/each}
			</SelectContent>
		</Select>
		<div class="flex-1"></div>
		<Badge variant="secondary">
			Page {pageIndex + 1}{nextToken ? '+' : ''}
		</Badge>
		<Button
			variant="ghost"
			size="icon-sm"
			onclick={prevPage}
			disabled={pageStack.length === 0 || loading}
			title="Previous page"
		>
			<ChevronLeft class="size-4" />
		</Button>
		<Button
			variant="ghost"
			size="icon-sm"
			onclick={nextPage}
			disabled={!nextToken || loading}
			title="Next page"
		>
			<ChevronRight class="size-4" />
		</Button>
		<Button
			variant="ghost"
			size="icon-sm"
			onclick={() => void reset()}
			disabled={loading}
			title="Refresh"
		>
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
		<Button variant="outline" size="xs" onclick={() => (importOpen = true)}>Import CSV</Button>
		<Button size="xs" onclick={() => (createUserOpen = true)}>
			<Plus class="size-3.5" /> User
		</Button>
	</div>

	<div class="flex-1 overflow-y-auto px-6 py-4">
		{#if loading && users.length === 0}
			<p class="text-xs text-muted-foreground">Loading users...</p>
		{:else if users.length === 0 && filter.trim()}
			<p class="text-xs text-muted-foreground">
				No users match "{filter}". Filter searches by username prefix.
			</p>
		{:else if users.length === 0}
			<p class="text-xs text-muted-foreground">No users in this pool.</p>
		{:else}
			<ul class="space-y-1.5">
				{#each users as u (u.username)}
					<li class="rounded border border-border/60">
						<div class="flex flex-wrap items-center gap-2 px-3 py-2 text-sm">
							<button
								type="button"
								class="flex min-w-0 flex-1 items-center gap-1.5 text-left"
								onclick={() =>
									(expandedUser = expandedUser === u.username ? null : u.username)}
								aria-expanded={expandedUser === u.username}
							>
								<ChevronRight
									class="size-3.5 shrink-0 text-muted-foreground transition-transform {expandedUser ===
									u.username
										? 'rotate-90'
										: ''}"
								/>
								<div class="min-w-0">
									<div class="flex flex-wrap items-center gap-2">
										<span class="truncate font-medium">{u.username}</span>
										<Badge variant={u.enabled ? 'secondary' : 'destructive'}>
											{u.enabled ? 'enabled' : 'disabled'}
										</Badge>
										<Badge variant="outline" class="font-mono text-[10px]">{u.status}</Badge>
									</div>
									<div class="text-xs text-muted-foreground">{u.createDate}</div>
								</div>
							</button>
							<div class="flex shrink-0 flex-wrap gap-1">
								<Button variant="ghost" size="xs" onclick={() => toggleEnabled(u)}>
									{u.enabled ? 'Disable' : 'Enable'}
								</Button>
								{#if u.status === 'UNCONFIRMED'}
									<Button variant="ghost" size="xs" onclick={() => confirmUser(u)}>
										Confirm
									</Button>
								{/if}
								<Button variant="ghost" size="xs" onclick={() => resetPassword(u)}>
									Reset PW
								</Button>
								<Button variant="ghost" size="xs" onclick={() => openSetPassword(u.username)}>
									Set PW
								</Button>
								<Button
									variant="ghost"
									size="xs"
									class="text-destructive hover:text-destructive"
									onclick={() => openDelete(u.username)}
								>
									Delete
								</Button>
							</div>
						</div>
						{#if expandedUser === u.username}
							<div class="border-t border-border/60 px-3 py-3">
								{#key u.username}
									<UserDetail {poolId} username={u.username} />
								{/key}
							</div>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>

<CreateUserDialog
	bind:open={createUserOpen}
	{poolId}
	{pool}
	onClose={() => (createUserOpen = false)}
	onCreated={() => void fetchPage(currentToken)}
/>
<ImportUsersDialog
	bind:open={importOpen}
	{poolId}
	onClose={() => (importOpen = false)}
	onComplete={() => void fetchPage(currentToken)}
/>
{#if setPwUser}
	<SetPasswordDialog
		bind:open={setPwOpen}
		{poolId}
		username={setPwUser}
		onClose={() => {
			setPwOpen = false;
			setPwUser = null;
		}}
	/>
{/if}
{#if deleteUser}
	<ConfirmDialog
		bind:open={deleteOpen}
		title="Delete user"
		description={`Permanently delete ${deleteUser}? This cannot be undone.`}
		busy={deleteBusy}
		onConfirm={confirmDeleteUser}
		onClose={() => {
			deleteOpen = false;
			deleteUser = null;
		}}
	/>
{/if}
