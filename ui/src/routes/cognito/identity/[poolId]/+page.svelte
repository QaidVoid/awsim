<script lang="ts">
	import { page } from '$app/state';
	import { goto, replaceState } from '$app/navigation';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		describeIdentityPool,
		getIdentityPoolRoles,
		listIdentities,
		listTagsForResource,
		deleteIdentityPool,
		type IdentityPoolDetail,
		type IdentityPoolIdentity,
		type IdentityPoolRoles
	} from '$lib/api/cognito';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { DataTable, EmptyState } from '$lib/components/service';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Fingerprint from '@lucide/svelte/icons/fingerprint';
	import Users from '@lucide/svelte/icons/users';
	import Shield from '@lucide/svelte/icons/shield';
	import Tags from '@lucide/svelte/icons/tags';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';

	const SECTIONS = [
		{ id: 'identities', label: 'Identities', icon: Users },
		{ id: 'roles', label: 'Roles', icon: Shield },
		{ id: 'tags', label: 'Tags', icon: Tags }
	] as const;

	type SectionId = (typeof SECTIONS)[number]['id'];
	const SECTION_IDS = SECTIONS.map((s) => s.id) as readonly string[];

	let poolId = $derived(page.params.poolId);
	let pool = $state<IdentityPoolDetail | null>(null);
	let identities = $state<IdentityPoolIdentity[]>([]);
	let roles = $state<IdentityPoolRoles | null>(null);
	let tags = $state<Record<string, string>>({});
	let loading = $state(true);
	let active = $state<SectionId>(initialSection());
	let showDeleteConfirm = $state(false);

	function initialSection(): SectionId {
		const tab = page.url.searchParams.get('section');
		return SECTION_IDS.includes(tab ?? '') ? (tab as SectionId) : 'identities';
	}

	$effect(() => {
		if (typeof window === 'undefined') return;
		const url = new URL(window.location.href);
		if (url.searchParams.get('section') === active) return;
		url.searchParams.set('section', active);
		replaceState(url.toString(), {});
	});

	$effect(() => {
		const id = poolId;
		if (id) void load(id);
	});

	async function load(id: string) {
		loading = true;
		try {
			const [p, i, r, t] = await Promise.all([
				describeIdentityPool(id),
				listIdentities(id),
				getIdentityPoolRoles(id).catch(() => null),
				listTagsForResource(
					`arn:aws:cognito-identity:us-east-1:000000000000:identitypool/${id}`
				).catch(() => ({} as Record<string, string>))
			]);
			pool = p;
			identities = i;
			roles = r;
			tags = t;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load identity pool');
		} finally {
			loading = false;
		}
	}

	async function handleDelete() {
		try {
			await deleteIdentityPool(poolId!);
			toast.success('Identity pool deleted');
			goto('/cognito');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}

	onMount(() => {
		if (poolId) void load(poolId);
	});
</script>

<div class="flex h-full min-h-0 flex-col overflow-hidden">
	<header class="flex items-center gap-3 border-b border-border bg-background px-6 py-3">
		<Button variant="ghost" size="icon-sm" onclick={() => goto('/cognito')} title="Back">
			<ArrowLeft class="size-4" />
		</Button>
		<div class="min-w-0 flex-1">
			<h1 class="truncate text-base font-semibold">{pool?.name ?? '—'}</h1>
			<code class="truncate text-xs text-muted-foreground">{poolId}</code>
		</div>
		{#if pool}
			<Badge variant={pool.allowUnauthenticated ? 'outline' : 'secondary'}>
				{pool.allowUnauthenticated ? 'Unauth enabled' : 'Unauth disabled'}
			</Badge>
		{/if}
		{#if loading}
			<span class="text-xs text-muted-foreground">Loading...</span>
		{/if}
	</header>

	<div class="flex flex-1 min-h-0 overflow-hidden">
		<nav
			class="flex w-56 shrink-0 flex-col gap-0.5 overflow-y-auto border-r border-border bg-muted/30 p-3"
		>
			{#each SECTIONS as s (s.id)}
				<button
					type="button"
					class="flex items-center gap-2 rounded px-3 py-2 text-left text-sm transition-colors {active ===
					s.id
						? 'bg-primary/15 font-medium text-primary'
						: 'text-muted-foreground hover:bg-muted hover:text-foreground'}"
					onclick={() => (active = s.id)}
				>
					<s.icon class="size-4 shrink-0" />
					{s.label}
				</button>
			{/each}

			<div class="mt-auto pt-4">
				{#if showDeleteConfirm}
					<div class="space-y-2 rounded border border-destructive/40 bg-destructive/5 p-3">
						<p class="text-xs text-destructive">Delete this identity pool?</p>
						<div class="flex gap-2">
							<Button variant="destructive" size="sm" onclick={handleDelete}>
								Delete
							</Button>
							<Button variant="ghost" size="sm" onclick={() => (showDeleteConfirm = false)}>
								Cancel
							</Button>
						</div>
					</div>
				{:else}
					<Button
						variant="ghost"
						size="sm"
						class="w-full text-destructive hover:bg-destructive/10"
						onclick={() => (showDeleteConfirm = true)}
					>
						<Trash2 class="mr-2 size-3.5" />
						Delete pool
					</Button>
				{/if}
			</div>
		</nav>

		<main class="flex min-w-0 flex-1 flex-col overflow-hidden">
			{#if poolId}
				{#key poolId}
					{#if active === 'identities'}
						<div class="flex h-full min-h-0 flex-col">
							<div class="flex items-center gap-2 border-b border-border px-6 py-3">
								<span class="text-sm text-muted-foreground">
									{identities.length} identit{identities.length === 1 ? 'y' : 'ies'}
								</span>
								<div class="flex-1"></div>
								<Button
									variant="ghost"
									size="icon-sm"
									onclick={() => poolId && listIdentities(poolId).then((i) => (identities = i))}
									title="Refresh"
								>
									<RefreshCw class="size-3.5" />
								</Button>
							</div>
							<div class="min-h-0 flex-1 overflow-hidden">
								<DataTable
									rows={identities}
									{loading}
									columns={[
										{ key: 'identityId', label: 'Identity ID', mono: true, width: '45%' },
										{ key: 'creationDate', label: 'Created', width: '30%' },
										{
											key: 'logins',
											label: 'Providers',
											width: '25%',
											cell: cellProviders
										}
									]}
									rowKey={(r: IdentityPoolIdentity) => r.identityId}
								>
									{#snippet empty()}
										<EmptyState
											icon={Fingerprint}
											title="No identities"
											description="Identities appear after users authenticate."
										/>
									{/snippet}
								</DataTable>
							</div>
						</div>
					{:else if active === 'roles'}
						<div class="w-full space-y-6 overflow-y-auto px-6 py-4">
							<section class="space-y-3">
								<h2 class="text-sm font-semibold uppercase tracking-wide text-muted-foreground">
									Auth / Unauth Roles
								</h2>
								{#if roles}
									<dl class="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
										<dt class="text-muted-foreground">Authenticated role</dt>
										<dd class="font-mono text-xs">
											{roles.authenticatedRoleArn ?? '—'}
										</dd>
										<dt class="text-muted-foreground">Unauthenticated role</dt>
										<dd class="font-mono text-xs">
											{roles.unauthenticatedRoleArn ?? '—'}
										</dd>
									</dl>
								{:else}
									<p class="text-xs text-muted-foreground">No roles configured.</p>
								{/if}
							</section>

							{#if pool?.cognitoIdentityProviders?.length}
								<section class="space-y-3">
									<h2 class="text-sm font-semibold uppercase tracking-wide text-muted-foreground">
										Linked Cognito providers
									</h2>
									<ul class="space-y-1.5">
										{#each pool.cognitoIdentityProviders as p (p.providerName + p.clientId)}
											<li class="rounded border border-border/60 px-3 py-2 font-mono text-xs">
												<div>{p.providerName}</div>
												<div class="text-muted-foreground">client: {p.clientId}</div>
											</li>
										{/each}
									</ul>
								</section>
							{/if}

							{#if pool?.developerProviderName}
								<section class="space-y-2">
									<h2 class="text-sm font-semibold uppercase tracking-wide text-muted-foreground">
										Developer provider
									</h2>
									<code class="text-xs">{pool.developerProviderName}</code>
								</section>
							{/if}
						</div>
					{:else if active === 'tags'}
						<div class="w-full overflow-y-auto px-6 py-4">
							{#if Object.keys(tags).length}
								<table class="w-full text-sm">
									<thead>
										<tr class="border-b border-border text-left text-xs text-muted-foreground">
											<th class="pb-2 font-medium">Key</th>
											<th class="pb-2 font-medium">Value</th>
										</tr>
									</thead>
									<tbody>
										{#each Object.entries(tags) as [key, value] (key)}
											<tr class="border-b border-border/50">
												<td class="py-2 font-mono text-xs">{key}</td>
												<td class="py-2 font-mono text-xs">{value}</td>
											</tr>
										{/each}
									</tbody>
								</table>
							{:else}
								<p class="text-xs text-muted-foreground">No tags configured.</p>
							{/if}
						</div>
					{/if}
				{/key}
			{/if}
		</main>
	</div>
</div>

{#snippet cellProviders(r: IdentityPoolIdentity)}
	{#if Object.keys(r.logins).length}
		{#each Object.keys(r.logins) as provider}
			<Badge variant="secondary" class="mr-1 text-[10px]">{provider}</Badge>
		{/each}
	{:else}
		<span class="text-xs text-muted-foreground">—</span>
	{/if}
{/snippet}
