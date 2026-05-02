<script lang="ts">
	import { page } from '$app/state';
	import { goto, replaceState } from '$app/navigation';
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
	import PolicyEditor from '$lib/components/iam/policy-editor.svelte';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
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

	async function handleDelete() {
		if (!confirm(`Delete policy "${policy?.policyName ?? policyArn}"? This cannot be undone.`)) return;
		try {
			await deletePolicy(policyArn);
			toast.success('Deleted policy');
			goto('/iam');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col overflow-hidden">
	<header class="flex items-center gap-3 border-b border-border bg-background px-6 py-3">
		<Button variant="ghost" size="icon-sm" onclick={() => goto('/iam')} title="Back to IAM">
			<ArrowLeft class="size-4" />
		</Button>
		<div class="min-w-0 flex-1">
			<h1 class="truncate text-base font-semibold">{policy?.policyName ?? '—'}</h1>
			<code class="truncate text-xs text-muted-foreground">{policyArn}</code>
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
		</nav>

		<main class="flex min-w-0 flex-1 flex-col overflow-hidden">
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
		</main>
	</div>
</div>
