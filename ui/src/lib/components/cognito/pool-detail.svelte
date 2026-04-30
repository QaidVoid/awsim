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
	import CreateUserDialog from './create-user-dialog.svelte';
	import SetPasswordDialog from './set-password-dialog.svelte';
	import ConfirmDialog from './confirm-dialog.svelte';

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
			const [d, u, g, c] = await Promise.all([
				describeUserPool(p.id),
				listPoolUsers(p.id),
				listGroups(p.id),
				listAppClients(p.id)
			]);
			detail = d;
			users = u;
			groups = g;
			clients = c;
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
			users = await listPoolUsers(pool.id);
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

	let userFilter = $state('');
	const filteredUsers = $derived(
		userFilter.trim()
			? users.filter((u) =>
					u.username.toLowerCase().includes(userFilter.trim().toLowerCase())
				)
			: users
	);

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
			users = await listPoolUsers(pool.id);
		} finally {
			loadingUsers = false;
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
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent side="right" class="w-full overflow-y-auto sm:max-w-[min(900px,90vw)]">
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
					{:else if users.length === 0}
						<p class="text-xs text-muted-foreground">No users in this pool.</p>
					{:else if filteredUsers.length === 0}
						<p class="text-xs text-muted-foreground">No users match "{userFilter}".</p>
					{:else}
						<ul class="space-y-1.5">
							{#each filteredUsers as u (u.username)}
								<li
									class="flex flex-wrap items-center gap-2 rounded border border-border/60 px-3 py-2 text-sm"
								>
									<div class="min-w-0 flex-1">
										<div class="flex flex-wrap items-center gap-2">
											<span class="truncate font-medium">{u.username}</span>
											<Badge variant={u.enabled ? 'secondary' : 'destructive'}>
												{u.enabled ? 'enabled' : 'disabled'}
											</Badge>
											<Badge variant="outline" class="font-mono text-[10px]">{u.status}</Badge>
										</div>
										<div class="text-xs text-muted-foreground">{u.createDate}</div>
									</div>
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
								</li>
							{/each}
						</ul>
					{/if}
				</TabsContent>

				<TabsContent value="groups" class="mt-4">
					{#if loadingGroups}
						<p class="text-xs text-muted-foreground">Loading groups...</p>
					{:else if groups.length === 0}
						<p class="text-xs text-muted-foreground">No groups.</p>
					{:else}
						<ul class="space-y-1.5">
							{#each groups as g (g.name)}
								<li class="rounded border border-border/60 px-3 py-2 text-sm">
									<div class="font-medium">{g.name}</div>
									{#if g.description}
										<div class="text-xs text-muted-foreground">{g.description}</div>
									{/if}
									{#if g.roleArn}
										<div class="font-mono text-xs text-muted-foreground">{g.roleArn}</div>
									{/if}
								</li>
							{/each}
						</ul>
					{/if}
				</TabsContent>

				<TabsContent value="clients" class="mt-4">
					{#if loadingClients}
						<p class="text-xs text-muted-foreground">Loading clients...</p>
					{:else if clients.length === 0}
						<p class="text-xs text-muted-foreground">No app clients.</p>
					{:else}
						<ul class="space-y-1.5">
							{#each clients as c (c.clientId)}
								<li class="rounded border border-border/60 px-3 py-2 text-sm">
									<div class="font-medium">{c.clientName}</div>
									<div class="font-mono text-xs text-muted-foreground">{c.clientId}</div>
								</li>
							{/each}
						</ul>
					{/if}
				</TabsContent>

				<TabsContent value="domain" class="mt-4">
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
							{#if domain}
								<dt class="text-muted-foreground">Domain</dt>
								<dd class="col-span-2 font-mono text-xs">{domain.domain}</dd>
							{/if}
						</dl>
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
{/if}
