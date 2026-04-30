<script lang="ts">
	import {
		describeUserPool,
		listPoolUsers,
		listGroups,
		listAppClients,
		describeDomain,
		adminEnableUser,
		adminDisableUser,
		adminConfirmSignUp,
		adminResetUserPassword,
		adminDeleteUser,
		deleteGroup,
		deleteAppClient,
		createDomain,
		deleteDomain,
		type UserPool,
		type UserPoolDetail,
		type CognitoUserSummary,
		type CognitoGroup,
		type CognitoAppClient,
		type CognitoDomain
	} from '$lib/api/cognito';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { toast } from 'svelte-sonner';
	import Plus from '@lucide/svelte/icons/plus';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import CreateUserDialog from './create-user-dialog.svelte';
	import SetPasswordDialog from './set-password-dialog.svelte';
	import ConfirmDialog from './confirm-dialog.svelte';
	import UserDetail from './user-detail.svelte';
	import CreateGroupDialog from './create-group-dialog.svelte';
	import GroupDetail from './group-detail.svelte';
	import ClientDetail from './client-detail.svelte';
	import CreateClientDialog from './create-client-dialog.svelte';
	import TriggersTab from './triggers-tab.svelte';

	interface Props {
		pool: UserPool | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { pool, open = $bindable(), onOpenChange }: Props = $props();

	let active = $state('users');
	let detail = $state<UserPoolDetail | null>(null);
	let users = $state<CognitoUserSummary[]>([]);
	let groups = $state<CognitoGroup[]>([]);
	let clients = $state<CognitoAppClient[]>([]);
	let domain = $state<CognitoDomain | null>(null);

	let loadingDetail = $state(false);
	let loadingUsers = $state(false);
	let loadingGroups = $state(false);
	let loadingClients = $state(false);

	$effect(() => {
		if (pool && open) loadAll(pool);
	});

	async function loadAll(p: UserPool) {
		detail = null;
		users = [];
		groups = [];
		clients = [];
		domain = null;
		loadingDetail = true;
		loadingUsers = true;
		loadingGroups = true;
		loadingClients = true;
		try {
			const [d, page, g, cPage] = await Promise.all([
				describeUserPool(p.id),
				listPoolUsers(p.id, { limit: USERS_PAGE_SIZE }),
				listGroups(p.id),
				listAppClients(p.id, { maxResults: 60 })
			]);
			detail = d;
			users = page.users;
			usersNextToken = page.nextToken;
			groups = g;
			clients = cPage.clients;
			clientsNextToken = cPage.nextToken;
			if (d.domain) {
				try {
					domain = await describeDomain(d.domain);
				} catch {
					domain = { domain: d.domain };
				}
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load pool');
		} finally {
			loadingDetail = false;
			loadingUsers = false;
			loadingGroups = false;
			loadingClients = false;
		}
	}

	async function toggleEnabled(u: CognitoUserSummary) {
		if (!pool) return;
		try {
			if (u.enabled) await adminDisableUser(pool.id, u.username);
			else await adminEnableUser(pool.id, u.username);
			users = users.map((x) => (x.username === u.username ? { ...x, enabled: !x.enabled } : x));
			toast.success(`${u.username} ${u.enabled ? 'disabled' : 'enabled'}`);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Action failed');
		}
	}

	async function confirmUser(u: CognitoUserSummary) {
		if (!pool) return;
		try {
			await adminConfirmSignUp(pool.id, u.username);
			toast.success(`${u.username} confirmed`);
			await reloadUsers();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Confirm failed');
		}
	}

	async function resetPassword(u: CognitoUserSummary) {
		if (!pool) return;
		try {
			await adminResetUserPassword(pool.id, u.username);
			toast.success(`Reset triggered for ${u.username}`);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Reset failed');
		}
	}

	const USERS_PAGE_SIZE = 60;
	const USER_FILTER_DEBOUNCE_MS = 250;

	let userFilter = $state('');
	let usersNextToken = $state<string | undefined>(undefined);
	let loadingMoreUsers = $state(false);
	let userFilterTimer: ReturnType<typeof setTimeout> | null = null;

	/// Cognito Filter operators are restrictive: `=`, `^=`, plus `attribute "value"`.
	/// Map a free-form box into `username ^= "<input>"` so prefix searches Just Work.
	function buildUserFilter(raw: string): string | undefined {
		const t = raw.trim();
		if (!t) return undefined;
		const escaped = t.replace(/"/g, '\\"');
		return `username ^= "${escaped}"`;
	}

	$effect(() => {
		// React to filter changes only — pool changes are handled by loadAll.
		userFilter;
		if (!pool) return;
		if (userFilterTimer) clearTimeout(userFilterTimer);
		userFilterTimer = setTimeout(() => void reloadUsers(), USER_FILTER_DEBOUNCE_MS);
	});

	let expandedUser = $state<string | null>(null);
	let createUserOpen = $state(false);
	let setPwUser = $state<string | null>(null);
	let setPwOpen = $state(false);
	let deleteUser = $state<string | null>(null);
	let deleteUserOpen = $state(false);
	let deleteUserBusy = $state(false);

	async function reloadUsers() {
		if (!pool) return;
		loadingUsers = true;
		try {
			const page = await listPoolUsers(pool.id, {
				limit: USERS_PAGE_SIZE,
				filter: buildUserFilter(userFilter)
			});
			users = page.users;
			usersNextToken = page.nextToken;
		} finally {
			loadingUsers = false;
		}
	}

	async function loadMoreUsers() {
		if (!pool || !usersNextToken || loadingMoreUsers) return;
		loadingMoreUsers = true;
		try {
			const page = await listPoolUsers(pool.id, {
				limit: USERS_PAGE_SIZE,
				paginationToken: usersNextToken,
				filter: buildUserFilter(userFilter)
			});
			users = [...users, ...page.users];
			usersNextToken = page.nextToken;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Load more failed');
		} finally {
			loadingMoreUsers = false;
		}
	}

	function openSetPassword(username: string) {
		setPwUser = username;
		setPwOpen = true;
	}

	function openDelete(username: string) {
		deleteUser = username;
		deleteUserOpen = true;
	}

	async function confirmDeleteUser() {
		if (!pool || !deleteUser) return;
		deleteUserBusy = true;
		try {
			await adminDeleteUser(pool.id, deleteUser);
			toast.success(`Deleted ${deleteUser}`);
			deleteUserOpen = false;
			deleteUser = null;
			await reloadUsers();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteUserBusy = false;
		}
	}

	let expandedGroup = $state<string | null>(null);
	let createGroupOpen = $state(false);
	let deleteGroupName = $state<string | null>(null);
	let deleteGroupOpen = $state(false);
	let deleteGroupBusy = $state(false);

	async function reloadGroups() {
		if (!pool) return;
		loadingGroups = true;
		try {
			groups = await listGroups(pool.id);
		} finally {
			loadingGroups = false;
		}
	}

	function openDeleteGroup(name: string) {
		deleteGroupName = name;
		deleteGroupOpen = true;
	}

	async function confirmDeleteGroup() {
		if (!pool || !deleteGroupName) return;
		deleteGroupBusy = true;
		try {
			await deleteGroup(pool.id, deleteGroupName);
			toast.success(`Deleted group ${deleteGroupName}`);
			deleteGroupOpen = false;
			deleteGroupName = null;
			await reloadGroups();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteGroupBusy = false;
		}
	}

	let expandedClient = $state<string | null>(null);
	let createClientOpen = $state(false);
	let deleteClientId = $state<string | null>(null);
	let deleteClientName = $state<string | null>(null);
	let deleteClientOpen = $state(false);
	let deleteClientBusy = $state(false);

	let clientsNextToken = $state<string | undefined>(undefined);
	let loadingMoreClients = $state(false);

	async function reloadClients() {
		if (!pool) return;
		loadingClients = true;
		try {
			const page = await listAppClients(pool.id, { maxResults: 60 });
			clients = page.clients;
			clientsNextToken = page.nextToken;
		} finally {
			loadingClients = false;
		}
	}

	async function loadMoreClients() {
		if (!pool || !clientsNextToken || loadingMoreClients) return;
		loadingMoreClients = true;
		try {
			const page = await listAppClients(pool.id, {
				maxResults: 60,
				nextToken: clientsNextToken
			});
			clients = [...clients, ...page.clients];
			clientsNextToken = page.nextToken;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Load more failed');
		} finally {
			loadingMoreClients = false;
		}
	}

	function openDeleteClient(id: string, name: string) {
		deleteClientId = id;
		deleteClientName = name;
		deleteClientOpen = true;
	}

	async function confirmDeleteClient() {
		if (!pool || !deleteClientId) return;
		deleteClientBusy = true;
		try {
			await deleteAppClient(pool.id, deleteClientId);
			toast.success(`Deleted ${deleteClientName ?? deleteClientId}`);
			deleteClientOpen = false;
			deleteClientId = null;
			deleteClientName = null;
			await reloadClients();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteClientBusy = false;
		}
	}

	let domainInput = $state('');
	let domainBusy = $state(false);
	let deleteDomainOpen = $state(false);
	let deleteDomainBusy = $state(false);

	async function refreshDomain() {
		if (!pool) return;
		try {
			detail = await describeUserPool(pool.id);
			if (detail?.domain) {
				domain = (await describeDomain(detail.domain)) ?? { domain: detail.domain };
			} else {
				domain = null;
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Refresh failed');
		}
	}

	async function submitDomain() {
		if (!pool || !domainInput.trim()) return;
		domainBusy = true;
		try {
			await createDomain(pool.id, domainInput.trim());
			toast.success(`Domain ${domainInput.trim()} created`);
			domainInput = '';
			await refreshDomain();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Create domain failed');
		} finally {
			domainBusy = false;
		}
	}

	async function confirmDeleteDomain() {
		if (!pool || !domain) return;
		deleteDomainBusy = true;
		try {
			await deleteDomain(pool.id, domain.domain);
			toast.success('Domain deleted');
			deleteDomainOpen = false;
			await refreshDomain();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteDomainBusy = false;
		}
	}
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent
		side="right"
		class="w-full overflow-y-auto data-[side=right]:sm:max-w-[min(900px,90vw)]"
	>
		<SheetHeader>
			<SheetTitle>{pool?.name ?? ''}</SheetTitle>
			<SheetDescription class="font-mono text-xs">{pool?.id ?? ''}</SheetDescription>
		</SheetHeader>
		<div class="px-6 pb-6">
			<Tabs bind:value={active} class="mt-2">
				<TabsList variant="line">
					<TabsTrigger value="users">Users ({users.length})</TabsTrigger>
					<TabsTrigger value="groups">Groups ({groups.length})</TabsTrigger>
					<TabsTrigger value="clients">App Clients ({clients.length})</TabsTrigger>
					<TabsTrigger value="domain">Domain</TabsTrigger>
					<TabsTrigger value="triggers">Triggers</TabsTrigger>
				</TabsList>

				<TabsContent value="users" class="mt-4 space-y-3">
					<div class="flex items-center gap-2">
						<Input
							type="search"
							placeholder="Filter users..."
							bind:value={userFilter}
							class="h-8 max-w-xs"
						/>
						<div class="flex-1"></div>
						<Button variant="ghost" size="icon-sm" onclick={reloadUsers} title="Refresh">
							<RefreshCw class="size-3.5 {loadingUsers ? 'animate-spin' : ''}" />
						</Button>
						<Button size="xs" onclick={() => (createUserOpen = true)}>
							<Plus class="size-3.5" /> User
						</Button>
					</div>
					{#if loadingUsers}
						<p class="text-xs text-muted-foreground">Loading users...</p>
					{:else if users.length === 0 && userFilter.trim()}
						<p class="text-xs text-muted-foreground">
							No users match "{userFilter}". Filter searches usernames by prefix.
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
											aria-label="Toggle details for {u.username}"
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
									{#if expandedUser === u.username && pool}
										<div class="border-t border-border/60 px-3 py-3">
											{#key u.username}
												<UserDetail poolId={pool.id} username={u.username} />
											{/key}
										</div>
									{/if}
								</li>
							{/each}
						</ul>
						<div class="flex items-center justify-between text-xs text-muted-foreground">
							<span>
								Showing {users.length}{usersNextToken ? '+' : ''}
								{userFilter.trim() ? ` matching "${userFilter}"` : ''}
							</span>
							{#if usersNextToken}
								<Button
									variant="outline"
									size="xs"
									onclick={loadMoreUsers}
									disabled={loadingMoreUsers}
								>
									{loadingMoreUsers ? 'Loading...' : 'Load more'}
								</Button>
							{/if}
						</div>
					{/if}
				</TabsContent>

				<TabsContent value="groups" class="mt-4 space-y-3">
					<div class="flex items-center gap-2">
						<div class="flex-1"></div>
						<Button variant="ghost" size="icon-sm" onclick={reloadGroups} title="Refresh">
							<RefreshCw class="size-3.5 {loadingGroups ? 'animate-spin' : ''}" />
						</Button>
						<Button size="xs" onclick={() => (createGroupOpen = true)}>
							<Plus class="size-3.5" /> Group
						</Button>
					</div>
					{#if loadingGroups}
						<p class="text-xs text-muted-foreground">Loading groups...</p>
					{:else if groups.length === 0}
						<p class="text-xs text-muted-foreground">No groups.</p>
					{:else}
						<ul class="space-y-1.5">
							{#each groups as g (g.name)}
								<li class="rounded border border-border/60">
									<div class="flex flex-wrap items-center gap-2 px-3 py-2 text-sm">
										<button
											type="button"
											class="flex min-w-0 flex-1 items-center gap-1.5 text-left"
											onclick={() =>
												(expandedGroup = expandedGroup === g.name ? null : g.name)}
											aria-expanded={expandedGroup === g.name}
											aria-label="Toggle members for {g.name}"
										>
											<ChevronRight
												class="size-3.5 shrink-0 text-muted-foreground transition-transform {expandedGroup ===
												g.name
													? 'rotate-90'
													: ''}"
											/>
											<div class="min-w-0">
												<div class="flex flex-wrap items-center gap-2 font-medium">
													{g.name}
													{#if g.precedence !== undefined}
														<Badge variant="outline" class="font-mono text-[10px]">
															prec {g.precedence}
														</Badge>
													{/if}
												</div>
												{#if g.description}
													<div class="text-xs text-muted-foreground">{g.description}</div>
												{/if}
												{#if g.roleArn}
													<div class="truncate font-mono text-xs text-muted-foreground">
														{g.roleArn}
													</div>
												{/if}
											</div>
										</button>
										<Button
											variant="ghost"
											size="xs"
											class="text-destructive hover:text-destructive"
											onclick={() => openDeleteGroup(g.name)}
										>
											Delete
										</Button>
									</div>
									{#if expandedGroup === g.name && pool}
										<div class="border-t border-border/60 px-3 py-3">
											{#key g.name}
												<GroupDetail poolId={pool.id} groupName={g.name} />
											{/key}
										</div>
									{/if}
								</li>
							{/each}
						</ul>
					{/if}
				</TabsContent>

				<TabsContent value="clients" class="mt-4 space-y-3">
					<div class="flex items-center gap-2">
						<div class="flex-1"></div>
						<Button variant="ghost" size="icon-sm" onclick={reloadClients} title="Refresh">
							<RefreshCw class="size-3.5 {loadingClients ? 'animate-spin' : ''}" />
						</Button>
						<Button size="xs" onclick={() => (createClientOpen = true)}>
							<Plus class="size-3.5" /> Client
						</Button>
					</div>
					{#if loadingClients}
						<p class="text-xs text-muted-foreground">Loading clients...</p>
					{:else if clients.length === 0}
						<p class="text-xs text-muted-foreground">No app clients.</p>
					{:else}
						<ul class="space-y-1.5">
							{#each clients as c (c.clientId)}
								<li class="rounded border border-border/60">
									<div class="flex flex-wrap items-center gap-2 px-3 py-2 text-sm">
										<button
											type="button"
											class="flex min-w-0 flex-1 items-center gap-1.5 text-left"
											onclick={() =>
												(expandedClient = expandedClient === c.clientId ? null : c.clientId)}
											aria-expanded={expandedClient === c.clientId}
											aria-label="Toggle details for {c.clientName}"
										>
											<ChevronRight
												class="size-3.5 shrink-0 text-muted-foreground transition-transform {expandedClient ===
												c.clientId
													? 'rotate-90'
													: ''}"
											/>
											<div class="min-w-0">
												<div class="truncate font-medium">{c.clientName}</div>
												<div class="truncate font-mono text-xs text-muted-foreground">
													{c.clientId}
												</div>
											</div>
										</button>
										<Button
											variant="ghost"
											size="xs"
											class="text-destructive hover:text-destructive"
											onclick={() => openDeleteClient(c.clientId, c.clientName)}
										>
											Delete
										</Button>
									</div>
									{#if expandedClient === c.clientId && pool}
										<div class="border-t border-border/60 px-3 py-3">
											{#key c.clientId}
												<ClientDetail poolId={pool.id} clientId={c.clientId} />
											{/key}
										</div>
									{/if}
								</li>
							{/each}
						</ul>
						{#if clientsNextToken}
							<div class="flex justify-center">
								<Button
									variant="outline"
									size="xs"
									onclick={loadMoreClients}
									disabled={loadingMoreClients}
								>
									{loadingMoreClients ? 'Loading...' : 'Load more clients'}
								</Button>
							</div>
						{/if}
					{/if}
				</TabsContent>

				<TabsContent value="domain" class="mt-4 space-y-4">
					{#if loadingDetail}
						<p class="text-xs text-muted-foreground">Loading...</p>
					{:else}
						<dl class="grid grid-cols-3 gap-x-4 gap-y-2 text-sm">
							<dt class="text-muted-foreground">Status</dt>
							<dd class="col-span-2">{detail?.status ?? '—'}</dd>
							<dt class="text-muted-foreground">Created</dt>
							<dd class="col-span-2">{detail?.creationDate ?? '—'}</dd>
							<dt class="text-muted-foreground">MFA</dt>
							<dd class="col-span-2">{detail?.mfaConfiguration ?? 'OFF'}</dd>
							<dt class="text-muted-foreground">Estimated users</dt>
							<dd class="col-span-2">{detail?.estimatedNumberOfUsers ?? 0}</dd>
						</dl>

						<div class="space-y-2 rounded border border-border/60 px-3 py-3">
							<div class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
								Hosted UI domain
							</div>
							{#if domain}
								<div class="flex flex-wrap items-center gap-2 text-sm">
									<code class="font-mono text-xs">{domain.domain}</code>
									{#if domain.status}
										<Badge variant="outline">{domain.status}</Badge>
									{/if}
									<div class="flex-1"></div>
									<Button
										variant="ghost"
										size="xs"
										class="text-destructive hover:text-destructive"
										onclick={() => (deleteDomainOpen = true)}
									>
										Delete
									</Button>
								</div>
							{:else}
								<form
									class="flex items-end gap-2"
									onsubmit={(e) => {
										e.preventDefault();
										void submitDomain();
									}}
								>
									<div class="flex-1 space-y-1">
										<label for="domain-input" class="text-xs text-muted-foreground">
											Domain prefix
										</label>
										<Input
											id="domain-input"
											bind:value={domainInput}
											placeholder="my-pool"
											class="h-8 font-mono text-xs"
											autocomplete="off"
										/>
									</div>
									<Button size="sm" type="submit" disabled={domainBusy || !domainInput.trim()}>
										Create
									</Button>
								</form>
							{/if}
						</div>
					{/if}
				</TabsContent>

				<TabsContent value="triggers" class="mt-4">
					{#if pool}
						{#key pool.id}
							<TriggersTab poolId={pool.id} />
						{/key}
					{/if}
				</TabsContent>
			</Tabs>
		</div>
	</SheetContent>
</Sheet>

{#if pool}
	<CreateUserDialog
		bind:open={createUserOpen}
		poolId={pool.id}
		onClose={() => (createUserOpen = false)}
		onCreated={() => void reloadUsers()}
	/>
	{#if setPwUser}
		<SetPasswordDialog
			bind:open={setPwOpen}
			poolId={pool.id}
			username={setPwUser}
			onClose={() => {
				setPwOpen = false;
				setPwUser = null;
			}}
		/>
	{/if}
	{#if deleteUser}
		<ConfirmDialog
			bind:open={deleteUserOpen}
			title="Delete user"
			description={`Permanently delete ${deleteUser}? This cannot be undone.`}
			busy={deleteUserBusy}
			onConfirm={confirmDeleteUser}
			onClose={() => {
				deleteUserOpen = false;
				deleteUser = null;
			}}
		/>
	{/if}
	<CreateGroupDialog
		bind:open={createGroupOpen}
		poolId={pool.id}
		onClose={() => (createGroupOpen = false)}
		onCreated={() => void reloadGroups()}
	/>
	{#if deleteGroupName}
		<ConfirmDialog
			bind:open={deleteGroupOpen}
			title="Delete group"
			description={`Delete group ${deleteGroupName}? Members are not deleted but lose this membership.`}
			busy={deleteGroupBusy}
			onConfirm={confirmDeleteGroup}
			onClose={() => {
				deleteGroupOpen = false;
				deleteGroupName = null;
			}}
		/>
	{/if}
	<CreateClientDialog
		bind:open={createClientOpen}
		poolId={pool.id}
		onClose={() => (createClientOpen = false)}
		onCreated={(id) => {
			void reloadClients();
			expandedClient = id;
		}}
	/>
	{#if deleteClientId}
		<ConfirmDialog
			bind:open={deleteClientOpen}
			title="Delete app client"
			description={`Delete client ${deleteClientName ?? deleteClientId}? Apps using this client ID will stop working.`}
			busy={deleteClientBusy}
			onConfirm={confirmDeleteClient}
			onClose={() => {
				deleteClientOpen = false;
				deleteClientId = null;
				deleteClientName = null;
			}}
		/>
	{/if}
	{#if domain}
		<ConfirmDialog
			bind:open={deleteDomainOpen}
			title="Delete domain"
			description={`Delete the hosted UI domain ${domain.domain}? Sign-ins to it will stop working.`}
			busy={deleteDomainBusy}
			onConfirm={confirmDeleteDomain}
			onClose={() => (deleteDomainOpen = false)}
		/>
	{/if}
{/if}
