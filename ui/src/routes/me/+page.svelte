<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { auth } from '$lib/auth-state.svelte';
	import { credentials } from '$lib/credentials.svelte';
	import {
		getUser,
		listAttachedUserPolicies,
		listUserPolicies,
		listGroupsForUser,
		listAccessKeys,
		getLoginProfile,
		type IamUser,
		type IamAttachedPolicy,
		type IamGroup,
		type IamAccessKey,
		type IamLoginProfile
	} from '$lib/api/iam';
	import { ServicePage, EmptyState } from '$lib/components/service';
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import ShieldUser from '@lucide/svelte/icons/shield-user';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import Users from '@lucide/svelte/icons/users';
	import FileBadge from '@lucide/svelte/icons/file-badge';
	import Copy from '@lucide/svelte/icons/copy';

	const userName = $derived(deriveUserName(auth.session?.principal));
	let user = $state<IamUser | null>(null);
	let attached = $state<IamAttachedPolicy[]>([]);
	let inline = $state<string[]>([]);
	let groups = $state<IamGroup[]>([]);
	let keys = $state<IamAccessKey[]>([]);
	let loginProfile = $state<IamLoginProfile | null>(null);
	let loading = $state(false);

	function deriveUserName(principal?: string): string {
		if (!principal) return '';
		const slash = principal.lastIndexOf('/');
		return slash >= 0 ? principal.slice(slash + 1) : principal;
	}

	async function load() {
		if (!userName) return;
		loading = true;
		try {
			const results = await Promise.allSettled([
				getUser(userName),
				listAttachedUserPolicies(userName),
				listUserPolicies(userName),
				listGroupsForUser(userName),
				listAccessKeys(userName),
				getLoginProfile(userName)
			]);
			user = results[0].status === 'fulfilled' ? results[0].value : null;
			attached = results[1].status === 'fulfilled' ? results[1].value : [];
			inline = results[2].status === 'fulfilled' ? results[2].value : [];
			groups = results[3].status === 'fulfilled' ? results[3].value : [];
			keys = results[4].status === 'fulfilled' ? results[4].value : [];
			loginProfile = results[5].status === 'fulfilled' ? results[5].value : null;
		} finally {
			loading = false;
		}
	}

	onMount(load);

	function copy(value: string, label: string) {
		navigator.clipboard
			.writeText(value)
			.then(() => toast.success(`${label} copied`))
			.catch(() => toast.error('Copy failed'));
	}
</script>

<ServicePage
	title="My identity"
	description={user?.arn ?? auth.session?.principal ?? '—'}
>
	{#if !auth.signedIn}
		<EmptyState
			icon={ShieldUser}
			title="Not signed in"
			description="Sign in to see your IAM identity, attached policies, and active credentials."
		/>
	{:else}
		<div class="flex flex-col gap-4 p-6">
			<Card>
				<CardHeader>
					<CardTitle class="flex items-center gap-2 text-sm">
						<ShieldUser class="size-4" /> Principal
					</CardTitle>
				</CardHeader>
				<CardContent>
					<dl class="grid grid-cols-3 gap-x-4 gap-y-1.5 text-sm">
						<dt class="text-muted-foreground">User name</dt>
						<dd class="col-span-2 font-mono text-xs">{userName || '—'}</dd>
						<dt class="text-muted-foreground">ARN</dt>
						<dd class="col-span-2 break-all font-mono text-xs">{user?.arn ?? '—'}</dd>
						<dt class="text-muted-foreground">User ID</dt>
						<dd class="col-span-2 font-mono text-xs">{user?.userId ?? '—'}</dd>
					</dl>
				</CardContent>
			</Card>

			<Card>
				<CardHeader>
					<CardTitle class="flex items-center gap-2 text-sm">
						<KeyRound class="size-4" /> Active credentials
					</CardTitle>
				</CardHeader>
				<CardContent class="space-y-3">
					{#if credentials.usingAdminFallback}
						<p class="text-xs text-muted-foreground">
							UI is signing requests with the admin bypass key. Policy evaluation is
							skipped for this session.
						</p>
					{:else}
						<div class="flex flex-col gap-1.5">
							<div class="text-[11px] uppercase text-muted-foreground">Access key ID</div>
							<div class="flex items-center gap-2">
								<code class="flex-1 truncate rounded-md border bg-muted/40 px-2 py-1.5 font-mono text-xs">
									{credentials.current().accessKeyId}
								</code>
								<Button
									variant="outline"
									size="icon-xs"
									onclick={() => copy(credentials.current().accessKeyId, 'Access key ID')}
								>
									<Copy />
								</Button>
							</div>
						</div>
					{/if}
					<div>
						<div class="text-[11px] uppercase text-muted-foreground">Console password</div>
						<p class="mt-1 text-xs">
							{#if loginProfile}
								Enabled. {loginProfile.passwordResetRequired
									? 'Password reset required on next sign-in.'
									: ''}
							{:else}
								Not set.
							{/if}
						</p>
					</div>
				</CardContent>
			</Card>

			<Card>
				<CardHeader>
					<CardTitle class="flex items-center gap-2 text-sm">
						<FileBadge class="size-4" /> Attached policies
						<Badge variant="outline" class="ml-2">{attached.length + inline.length}</Badge>
					</CardTitle>
				</CardHeader>
				<CardContent>
					{#if attached.length === 0 && inline.length === 0}
						<p class="text-xs text-muted-foreground">No policies attached directly.</p>
					{:else}
						<ul class="space-y-1 text-xs">
							{#each attached as p (p.policyArn)}
								<li class="flex items-center gap-2">
									<Badge variant="outline" class="text-[10px]">managed</Badge>
									<span class="font-mono">{p.policyName}</span>
									<span class="truncate text-muted-foreground">{p.policyArn}</span>
								</li>
							{/each}
							{#each inline as name (name)}
								<li class="flex items-center gap-2">
									<Badge variant="secondary" class="text-[10px]">inline</Badge>
									<span class="font-mono">{name}</span>
								</li>
							{/each}
						</ul>
					{/if}
				</CardContent>
			</Card>

			<Card>
				<CardHeader>
					<CardTitle class="flex items-center gap-2 text-sm">
						<Users class="size-4" /> Groups
						<Badge variant="outline" class="ml-2">{groups.length}</Badge>
					</CardTitle>
				</CardHeader>
				<CardContent>
					{#if groups.length === 0}
						<p class="text-xs text-muted-foreground">Not a member of any group.</p>
					{:else}
						<ul class="space-y-1 text-xs">
							{#each groups as g (g.arn)}
								<li class="flex items-center gap-2">
									<span class="font-mono">{g.groupName}</span>
									<span class="truncate text-muted-foreground">{g.arn}</span>
								</li>
							{/each}
						</ul>
					{/if}
				</CardContent>
			</Card>

			<Card>
				<CardHeader>
					<CardTitle class="flex items-center gap-2 text-sm">
						<KeyRound class="size-4" /> Access keys
						<Badge variant="outline" class="ml-2">{keys.length} / 2</Badge>
					</CardTitle>
				</CardHeader>
				<CardContent>
					{#if keys.length === 0}
						<p class="text-xs text-muted-foreground">No access keys.</p>
					{:else}
						<ul class="space-y-1 text-xs">
							{#each keys as k (k.accessKeyId)}
								<li class="flex items-center gap-2">
									<span class="font-mono">{k.accessKeyId}</span>
									<Badge
										variant={k.status === 'Active' ? 'default' : 'secondary'}
										class="text-[10px]"
									>
										{k.status}
									</Badge>
								</li>
							{/each}
						</ul>
					{/if}
				</CardContent>
			</Card>
		</div>
	{/if}
</ServicePage>
