<script lang="ts">
	import { page } from '$app/state';
	import { goto, replaceState } from '$app/navigation';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { describeUserPool, type UserPool } from '$lib/api/cognito';
	import { Button } from '$lib/components/ui/button';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Users from '@lucide/svelte/icons/users';
	import UsersRound from '@lucide/svelte/icons/users-round';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import Globe from '@lucide/svelte/icons/globe';
	import Zap from '@lucide/svelte/icons/zap';
	import Shield from '@lucide/svelte/icons/shield';
	import Network from '@lucide/svelte/icons/network';
	import Palette from '@lucide/svelte/icons/palette';

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

	const SECTIONS = [
		{ id: 'users', label: 'Users', icon: Users },
		{ id: 'groups', label: 'Groups', icon: UsersRound },
		{ id: 'clients', label: 'App clients', icon: KeyRound },
		{ id: 'domain', label: 'Domain', icon: Globe },
		{ id: 'triggers', label: 'Triggers', icon: Zap },
		{ id: 'policies', label: 'Policies', icon: Shield },
		{ id: 'federation', label: 'Federation', icon: Network },
		{ id: 'appearance', label: 'Appearance', icon: Palette }
	] as const;

	type SectionId = (typeof SECTIONS)[number]['id'];
	const SECTION_IDS = SECTIONS.map((s) => s.id) as readonly string[];

	let poolId = $derived(page.params.poolId);
	let pool = $state<UserPool | null>(null);
	let loading = $state(true);
	let active = $state<SectionId>(initialSection());

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

<div class="flex h-full min-h-0 flex-col overflow-hidden">
	<!-- Pool header -->
	<header class="flex items-center gap-3 border-b border-border bg-background px-6 py-3">
		<Button variant="ghost" size="icon-sm" onclick={() => goto('/cognito')} title="Back to pools">
			<ArrowLeft class="size-4" />
		</Button>
		<div class="min-w-0 flex-1">
			<h1 class="truncate text-base font-semibold">{pool?.name ?? '—'}</h1>
			<code class="truncate text-xs text-muted-foreground">{poolId}</code>
		</div>
		{#if loading}
			<span class="text-xs text-muted-foreground">Loading...</span>
		{/if}
	</header>

	<div class="flex flex-1 min-h-0 overflow-hidden">
		<!-- Left nav -->
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
		</nav>

		<!-- Section content -->
		<main class="flex min-w-0 flex-1 overflow-hidden">
			{#if poolId}
				{#key poolId}
					{#if active === 'users'}
						<UsersSection {poolId} />
					{:else if active === 'groups'}
						<GroupsSection {poolId} />
					{:else if active === 'clients'}
						<ClientsSection {poolId} />
					{:else if active === 'domain'}
						<div class="w-full overflow-y-auto">
							<DomainSection {poolId} />
						</div>
					{:else if active === 'triggers'}
						<div class="w-full overflow-y-auto px-6 py-4">
							<TriggersTab {poolId} />
						</div>
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
		</main>
	</div>
</div>
