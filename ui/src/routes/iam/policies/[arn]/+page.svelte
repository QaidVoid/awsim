<script lang="ts">
	import { page } from '$app/state';
	import { goto, replaceState } from '$app/navigation';
	import { route } from '$lib/url';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		getPolicy,
		listPolicyVersions,
		getPolicyVersion,
		createPolicyVersion,
		setDefaultPolicyVersion,
		deletePolicy,
		type IamPolicy,
		type IamPolicyVersion
	} from '$lib/api/iam';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import { DetailPage, DetailNavItem } from '$lib/components/service';
	import PolicyEditor from '$lib/components/iam/policy-editor.svelte';
	import FileBadge from '@lucide/svelte/icons/file-badge';
	import Trash2 from '@lucide/svelte/icons/trash-2';

	const SECTIONS = [
		{ id: 'document', label: 'Policy document', icon: FileBadge }
	] as const;

	type SectionId = (typeof SECTIONS)[number]['id'];

	let policyArn = $derived(decodeURIComponent(page.params.arn ?? ''));
	let policy = $state<IamPolicy | null>(null);
	let loading = $state(true);
	let active = $state<SectionId>('document');
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	$effect(() => {
		if (typeof window === 'undefined') return;
		const url = new URL(window.location.href);
		if (url.searchParams.get('section') === active) return;
		url.searchParams.set('section', active);
		replaceState(url.toString(), {});
	});

	let versions = $state<IamPolicyVersion[]>([]);
	let activeVersionId = $state<string | null>(null);
	let policyDoc = $state('');
	let saving = $state(false);

	async function load() {
		loading = true;
		try {
			const [detail, vers] = await Promise.all([getPolicy(policyArn), listPolicyVersions(policyArn)]);
			policy = detail;
			versions = vers;
			const def = vers.find((v) => v.isDefaultVersion) ?? vers[0];
			if (def) await loadVersion(def.versionId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load policy');
		} finally {
			loading = false;
		}
	}

	onMount(load);

	async function loadVersion(versionId: string) {
		activeVersionId = versionId;
		try {
			const v = await getPolicyVersion(policyArn, versionId);
			try {
				policyDoc = JSON.stringify(JSON.parse(v.document), null, 2);
			} catch {
				policyDoc = v.document;
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load version');
		}
	}

	async function saveAsNewVersion() {
		saving = true;
		try {
			await createPolicyVersion(policyArn, policyDoc, true);
			toast.success('Saved new policy version');
			versions = await listPolicyVersions(policyArn);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to save version');
		} finally {
			saving = false;
		}
	}

	async function setDefault(versionId: string) {
		try {
			await setDefaultPolicyVersion(policyArn, versionId);
			toast.success(`Set ${versionId} as default`);
			versions = await listPolicyVersions(policyArn);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to set default');
		}
	}

	function handleDelete() {
		deleteOpen = true;
	}

	async function confirmDeletePolicy() {
		deleteBusy = true;
		try {
			await deletePolicy(policyArn);
			toast.success('Deleted policy');
			goto(route('/iam'));
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}
</script>

<DetailPage
	title={policy?.policyName ?? '—'}
	subtitle={policyArn}
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

		{#if policy}
			<div class="mt-4 border-t border-border pt-3">
				<div class="px-3 text-xs text-muted-foreground">Details</div>
				{#if policy.description}
					<div class="mt-2 px-3 text-xs">{policy.description}</div>
				{/if}
				<div class="mt-1 px-3 text-xs text-muted-foreground">Attachments: {policy.attachmentCount}</div>
				<div class="mt-1 px-3 text-xs text-muted-foreground">Default: {policy.defaultVersionId ?? '—'}</div>
			</div>
		{/if}

		<div class="flex-1"></div>

		<button
			type="button"
			class="flex items-center gap-2 rounded px-3 py-2 text-left text-sm text-destructive transition-colors hover:bg-destructive/10"
			onclick={handleDelete}
		>
			<Trash2 class="size-4 shrink-0" />
			Delete policy
		</button>
	{/snippet}

	{#if active === 'document'}
				<div class="overflow-y-auto p-6">
					<div class="mb-3">
						<h2 class="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
							Versions
						</h2>
						<div class="mb-3 flex flex-wrap gap-1.5">
							{#each versions as v (v.versionId)}
								<button
									type="button"
									class="rounded border px-2 py-1 font-mono text-xs transition-colors {activeVersionId === v.versionId
										? 'border-primary bg-primary/10 text-primary'
										: 'border-border hover:bg-muted'}"
									onclick={() => loadVersion(v.versionId)}
								>
									{v.versionId}{v.isDefaultVersion ? ' (default)' : ''}
								</button>
							{/each}
						</div>
						{#if activeVersionId && !versions.find((v) => v.versionId === activeVersionId)?.isDefaultVersion}
							<Button variant="outline" size="xs" onclick={() => activeVersionId && setDefault(activeVersionId)}>
								Set {activeVersionId} as default
							</Button>
						{/if}
					</div>
					<PolicyEditor bind:value={policyDoc} id="policy-doc" label="Policy document" rows={18} />
					<div class="mt-2 flex justify-end">
						<Button size="sm" onclick={saveAsNewVersion} disabled={saving || !policyDoc}>
							{saving ? 'Saving...' : 'Save as new version'}
						</Button>
					</div>
				</div>
			{/if}
</DetailPage>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete policy?"
	description={`Permanently delete "${policy?.policyName ?? policyArn}". This cannot be undone.`}
	busy={deleteBusy}
	onConfirm={confirmDeletePolicy}
	onClose={() => (deleteOpen = false)}
/>
