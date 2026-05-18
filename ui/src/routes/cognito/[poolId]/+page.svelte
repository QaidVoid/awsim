<script lang="ts">
	import { page } from '$app/state';
	import { goto, replaceState } from '$app/navigation';
	import { route } from '$lib/url';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { describeUserPool, type UserPoolDetail } from '$lib/api/cognito';
	import { DetailPage, DetailNavItem } from '$lib/components/service';
	import Users from '@lucide/svelte/icons/users';
	import UsersRound from '@lucide/svelte/icons/users-round';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import LogIn from '@lucide/svelte/icons/log-in';
	import Globe from '@lucide/svelte/icons/globe';
	import Zap from '@lucide/svelte/icons/zap';
	import Shield from '@lucide/svelte/icons/shield';
	import Network from '@lucide/svelte/icons/network';
	import Palette from '@lucide/svelte/icons/palette';
	import Tag from '@lucide/svelte/icons/tag';

	import UsersSection from '$lib/components/cognito/users-section.svelte';
	import GroupsSection from '$lib/components/cognito/groups-section.svelte';
	import ClientsSection from '$lib/components/cognito/clients-section.svelte';
	import DomainSection from '$lib/components/cognito/domain-section.svelte';
	import TriggersTab from '$lib/components/cognito/triggers-tab.svelte';
	import PasswordPolicyEditor from '$lib/components/cognito/password-policy-editor.svelte';
	import MfaConfigEditor from '$lib/components/cognito/mfa-config-editor.svelte';
	import TagsEditor from '$lib/components/cognito/tags-editor.svelte';
	import IdpTab from '$lib/components/cognito/idp-tab.svelte';
	import ResourceServersTab from '$lib/components/cognito/resource-servers-tab.svelte';
	import AppearanceSection from '$lib/components/cognito/appearance-section.svelte';
	import AttributesTab from '$lib/components/cognito/attributes-tab.svelte';
	import AuthenticateSection from '$lib/components/cognito/authenticate-section.svelte';

	const SECTIONS = [
		{ id: 'users', label: 'Users', icon: Users },
		{ id: 'groups', label: 'Groups', icon: UsersRound },
		{ id: 'clients', label: 'App clients', icon: KeyRound },
		{ id: 'authenticate', label: 'Sign in', icon: LogIn },
		{ id: 'attributes', label: 'Attributes', icon: Tag },
		{ id: 'domain', label: 'Domain', icon: Globe },
		{ id: 'triggers', label: 'Triggers', icon: Zap },
		{ id: 'policies', label: 'Policies', icon: Shield },
		{ id: 'federation', label: 'Federation', icon: Network },
		{ id: 'appearance', label: 'Appearance', icon: Palette }
	] as const;

	type SectionId = (typeof SECTIONS)[number]['id'];
	const SECTION_IDS = SECTIONS.map((s) => s.id) as readonly string[];

	let poolId = $derived(page.params.poolId);
	let pool = $state<UserPoolDetail | null>(null);
	let loading = $state(true);
	let active = $state<SectionId>(initialSection());
	let prefillUser = $state('');

	function signInAs(u: string) {
		prefillUser = u;
		active = 'authenticate';
	}

	function initialSection(): SectionId {
		const tab = page.url.searchParams.get('section');
		return (SECTION_IDS.includes(tab ?? '') ? (tab as SectionId) : 'users');
	}

	$effect(() => {
		// Sync section to URL so it's bookmarkable + survives refresh.
		if (typeof window === 'undefined') return;
		const url = new URL(window.location.href);
		if (url.searchParams.get('section') === active) return;
		url.searchParams.set('section', active);
		replaceState(url.toString(), {});
	});

	$effect(() => {
		// Re-load if the route param changes (e.g. user navigates between pools).
		const id = poolId;
		if (id) void loadPool(id);
	});

	async function loadPool(id: string) {
		loading = true;
		try {
			pool = await describeUserPool(id);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load pool');
		} finally {
			loading = false;
		}
	}

	onMount(() => {
		// $effect picks this up too, but onMount keeps the loading flag
		// from flickering when the param is read synchronously.
		if (poolId) void loadPool(poolId);
	});
</script>

<DetailPage
	title={pool?.name ?? '—'}
	subtitle={poolId}
	backHref="/cognito"
	backLabel="Back to pools"
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
	{/snippet}
			{#if poolId}
				{#key poolId}
					{#if active === 'users'}
						<UsersSection {poolId} {pool} onSignIn={signInAs} />
					{:else if active === 'groups'}
						<GroupsSection {poolId} />
					{:else if active === 'clients'}
						<ClientsSection {poolId} />
					{:else if active === 'authenticate'}
						<AuthenticateSection {poolId} {prefillUser} />
					{:else if active === 'attributes'}
						<AttributesTab {pool} onRefresh={() => loadPool(poolId)} />
					{:else if active === 'domain'}
						<div class="w-full overflow-y-auto">
							<DomainSection {poolId} />
						</div>
					{:else if active === 'triggers'}
						<TriggersTab {poolId} />
					{:else if active === 'policies'}
						<div class="w-full space-y-4 overflow-y-auto px-6 py-4">
							<PasswordPolicyEditor {poolId} />
							<MfaConfigEditor {poolId} />
							<TagsEditor {poolId} />
						</div>
					{:else if active === 'federation'}
						<div class="w-full space-y-6 overflow-y-auto px-6 py-4">
							<section class="space-y-2">
								<h2 class="text-sm font-semibold uppercase tracking-wide text-muted-foreground">
									Identity providers
								</h2>
								<IdpTab {poolId} />
							</section>
							<section class="space-y-2">
								<h2 class="text-sm font-semibold uppercase tracking-wide text-muted-foreground">
									Resource servers
								</h2>
								<ResourceServersTab {poolId} />
							</section>
						</div>
					{:else if active === 'appearance'}
						<AppearanceSection {poolId} />
					{/if}
				{/key}
			{/if}
</DetailPage>
