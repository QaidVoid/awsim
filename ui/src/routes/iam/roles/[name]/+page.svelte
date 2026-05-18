<script lang="ts">
	import { page } from '$app/state';
	import { goto, replaceState } from '$app/navigation';
	import { route } from '$lib/url';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		getRole,
		deleteRole,
		listAttachedRolePolicies,
		attachRolePolicy,
		detachRolePolicy,
		listRolePolicies,
		getRolePolicy,
		putRolePolicy,
		deleteRolePolicy,
		updateAssumeRolePolicy,
		type IamRole,
		type IamAttachedPolicy
	} from '$lib/api/iam';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import EntityPoliciesEditor from '$lib/components/iam/entity-policies-editor.svelte';
	import PolicyEditor from '$lib/components/iam/policy-editor.svelte';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Shield from '@lucide/svelte/icons/shield';
	import FileBadge from '@lucide/svelte/icons/file-badge';
	import Save from '@lucide/svelte/icons/save';
	import Trash2 from '@lucide/svelte/icons/trash-2';

	const SECTIONS = [
		{ id: 'trust', label: 'Trust policy', icon: Shield },
		{ id: 'policies', label: 'Policies', icon: FileBadge }
	] as const;

	type SectionId = (typeof SECTIONS)[number]['id'];
	const SECTION_IDS = SECTIONS.map((s) => s.id) as readonly string[];

	let roleName = $derived(decodeURIComponent(page.params.name ?? ''));
	let role = $state<IamRole | null>(null);
	let loading = $state(true);
	let active = $state<SectionId>(initialSection());
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	function initialSection(): SectionId {
		const tab = page.url.searchParams.get('section');
		return SECTION_IDS.includes(tab ?? '') ? (tab as SectionId) : 'trust';
	}

	$effect(() => {
		if (typeof window === 'undefined') return;
		const url = new URL(window.location.href);
		if (url.searchParams.get('section') === active) return;
		url.searchParams.set('section', active);
		replaceState(url.toString(), {});
	});

	let trustDoc = $state('');
	let trustDocOriginal = $state('');
	let savingTrust = $state(false);
	const trustModified = $derived(trustDoc.trim() !== trustDocOriginal.trim());

	async function load() {
		loading = true;
		try {
			const detail = await getRole(roleName);
			role = detail;
			if (detail.assumeRolePolicyDocument) {
				try {
					trustDoc = JSON.stringify(JSON.parse(detail.assumeRolePolicyDocument), null, 2);
				} catch {
					trustDoc = detail.assumeRolePolicyDocument;
				}
				trustDocOriginal = trustDoc;
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load role');
		} finally {
			loading = false;
		}
	}

	onMount(load);

	async function saveTrustPolicy() {
		try {
			JSON.parse(trustDoc);
		} catch {
			toast.error('Trust policy is not valid JSON');
			return;
		}
		savingTrust = true;
		try {
			await updateAssumeRolePolicy(roleName, trustDoc);
			toast.success('Trust policy updated');
			trustDocOriginal = trustDoc;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		} finally {
			savingTrust = false;
		}
	}

	function handleDelete() {
		deleteOpen = true;
	}

	async function confirmDeleteRole() {
		deleteBusy = true;
		try {
			await deleteRole(roleName);
			toast.success(`Deleted ${roleName}`);
			goto(route('/iam'));
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}

	// Policies state
	let attached = $state<IamAttachedPolicy[]>([]);
	let inlineNames = $state<string[]>([]);
	let policiesLoaded = $state(false);

	async function reloadPolicies() {
		const [a, i] = await Promise.all([
			listAttachedRolePolicies(roleName).catch(() => []),
			listRolePolicies(roleName).catch(() => [])
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
		if (active === 'policies') loadPolicies();
	});
</script>

<div class="flex h-full min-h-0 flex-col overflow-hidden">
	<header class="flex items-center gap-3 border-b border-border bg-background px-6 py-3">
		<Button variant="ghost" size="icon-sm" onclick={() => goto(route('/iam'))} title="Back to IAM">
			<ArrowLeft class="size-4" />
		</Button>
		<div class="min-w-0 flex-1">
			<h1 class="truncate text-base font-semibold">{roleName}</h1>
			<code class="truncate text-xs text-muted-foreground">{role?.arn ?? '—'}</code>
		</div>
		{#if loading}
			<span class="text-xs text-muted-foreground">Loading...</span>
		{/if}
	</header>

	<div class="flex flex-1 min-h-0 overflow-hidden">
		<nav class="flex w-56 shrink-0 flex-col gap-0.5 overflow-y-auto border-r border-border bg-muted/30 p-3">
			{#each SECTIONS as s (s.id)}
				<button
					type="button"
					class="flex items-center gap-2 rounded px-3 py-2 text-left text-sm transition-colors {active === s.id
						? 'bg-primary/15 font-medium text-primary'
						: 'text-muted-foreground hover:bg-muted hover:text-foreground'}"
					onclick={() => (active = s.id)}
				>
					<s.icon class="size-4 shrink-0" />
					{s.label}
				</button>
			{/each}

			<div class="flex-1"></div>

			<button
				type="button"
				class="flex items-center gap-2 rounded px-3 py-2 text-left text-sm text-destructive transition-colors hover:bg-destructive/10"
				onclick={handleDelete}
			>
				<Trash2 class="size-4 shrink-0" />
				Delete role
			</button>
		</nav>

		<main class="flex min-w-0 flex-1 flex-col overflow-hidden">
			{#if role}
				{#if active === 'trust'}
					<div class="overflow-y-auto p-6">
						<dl class="mb-6 grid grid-cols-3 gap-x-4 gap-y-2 text-sm">
							<dt class="text-muted-foreground">Role ID</dt>
							<dd class="col-span-2 font-mono text-xs">{role.roleId}</dd>
							{#if role.description}
								<dt class="text-muted-foreground">Description</dt>
								<dd class="col-span-2">{role.description}</dd>
							{/if}
							<dt class="text-muted-foreground">ARN</dt>
							<dd class="col-span-2 break-all font-mono text-xs">{role.arn}</dd>
						</dl>
						<PolicyEditor bind:value={trustDoc} id="role-trust-policy" label="Trust policy" rows={14} />
						{#if trustModified}
							<div class="mt-2 flex justify-end">
								<Button size="sm" onclick={saveTrustPolicy} disabled={savingTrust}>
									<Save class="size-4" />
									<span class="ml-1">{savingTrust ? 'Saving…' : 'Save trust policy'}</span>
								</Button>
							</div>
						{/if}
					</div>
				{:else if active === 'policies'}
					<div class="overflow-y-auto p-6">
						<EntityPoliciesEditor
							{attached}
							{inlineNames}
							onAttach={(arn) => attachRolePolicy(roleName, arn)}
							onDetach={(arn) => detachRolePolicy(roleName, arn)}
							onLoadInline={(name) => getRolePolicy(roleName, name)}
							onPutInline={(name, doc) => putRolePolicy(roleName, name, doc)}
							onDeleteInline={(name) => deleteRolePolicy(roleName, name)}
							onMutated={reloadPolicies}
						/>
					</div>
				{/if}
			{/if}
		</main>
	</div>
</div>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete role?"
	description={`Permanently delete "${roleName}". This cannot be undone.`}
	busy={deleteBusy}
	onConfirm={confirmDeleteRole}
	onClose={() => (deleteOpen = false)}
/>
