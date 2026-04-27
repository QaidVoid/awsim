<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listPolicies,
		getPolicy,
		listPolicyVersions,
		getPolicyVersion,
		createPolicyVersion,
		setDefaultPolicyVersion,
		type IamPolicy,
		type IamPolicyVersion
	} from '$lib/api/iam';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import EntityDetailSheet from './entity-detail-sheet.svelte';
	import PolicyEditor from './policy-editor.svelte';
	import FileBadge from '@lucide/svelte/icons/file-badge';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import { toast } from 'svelte-sonner';

	let policies = $state<IamPolicy[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let selected = $state<IamPolicy | null>(null);
	let versions = $state<IamPolicyVersion[]>([]);
	let activeVersionId = $state<string | null>(null);
	let policyDoc = $state('');
	let detailLoading = $state(false);
	let saving = $state(false);

	const filtered = $derived(
		filter.trim()
			? policies.filter((p) => p.policyName.toLowerCase().includes(filter.trim().toLowerCase()))
			: policies
	);

	async function load() {
		loading = true;
		try {
			policies = await listPolicies('Local');
		} finally {
			loading = false;
		}
	}

	async function openDetail(p: IamPolicy) {
		selected = p;
		versions = [];
		policyDoc = '';
		activeVersionId = null;
		detailLoading = true;
		try {
			const [detail, vers] = await Promise.all([getPolicy(p.arn), listPolicyVersions(p.arn)]);
			selected = detail;
			versions = vers;
			const def = vers.find((v) => v.isDefaultVersion) ?? vers[0];
			if (def) await loadVersion(def.versionId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load policy');
		} finally {
			detailLoading = false;
		}
	}

	async function loadVersion(versionId: string) {
		if (!selected) return;
		activeVersionId = versionId;
		try {
			const v = await getPolicyVersion(selected.arn, versionId);
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
		if (!selected) return;
		saving = true;
		try {
			await createPolicyVersion(selected.arn, policyDoc, true);
			toast.success('Saved new policy version');
			const vers = await listPolicyVersions(selected.arn);
			versions = vers;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to save version');
		} finally {
			saving = false;
		}
	}

	async function setDefault(versionId: string) {
		if (!selected) return;
		try {
			await setDefaultPolicyVersion(selected.arn, versionId);
			toast.success(`Set ${versionId} as default`);
			versions = await listPolicyVersions(selected.arn);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to set default');
		}
	}

	onMount(load);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input
			type="search"
			placeholder="Filter policies..."
			bind:value={filter}
			class="h-8 max-w-xs"
		/>
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {policies.length}</Badge>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
	</div>

	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
			{loading}
			columns={[
				{ key: 'policyName', label: 'Policy name', width: '30%' },
				{ key: 'arn', label: 'ARN', mono: true },
				{
					key: 'attachmentCount',
					label: 'Attachments',
					width: '12%',
					align: 'right'
				}
			]}
			rowKey={(r: IamPolicy) => r.arn}
			onRowClick={openDetail}
		>
			{#snippet empty()}
				<EmptyState
					icon={FileBadge}
					title="No customer-managed policies"
					description="Create one with: aws iam create-policy --policy-name MyPolicy --policy-document file://p.json"
				/>
			{/snippet}
		</DataTable>
	</div>
</div>

<EntityDetailSheet
	open={!!selected}
	onOpenChange={(v) => {
		if (!v) selected = null;
	}}
	title={selected?.policyName ?? ''}
	subtitle={selected?.arn}
>
	{#if selected}
		<dl class="grid grid-cols-3 gap-x-4 gap-y-2 py-4 text-sm">
			{#if selected.description}
				<dt class="text-muted-foreground">Description</dt>
				<dd class="col-span-2">{selected.description}</dd>
			{/if}
			<dt class="text-muted-foreground">Default version</dt>
			<dd class="col-span-2 font-mono text-xs">{selected.defaultVersionId ?? '—'}</dd>
			<dt class="text-muted-foreground">Attachments</dt>
			<dd class="col-span-2">{selected.attachmentCount}</dd>
		</dl>
		<div class="mt-2">
			<h3 class="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
				Versions
			</h3>
			{#if detailLoading}
				<p class="text-xs text-muted-foreground">Loading...</p>
			{:else}
				<div class="mb-3 flex flex-wrap gap-1.5">
					{#each versions as v (v.versionId)}
						<button
							type="button"
							class="rounded border px-2 py-1 font-mono text-xs transition-colors {activeVersionId ===
							v.versionId
								? 'border-primary bg-primary/10 text-primary'
								: 'border-border hover:bg-muted'}"
							onclick={() => loadVersion(v.versionId)}
						>
							{v.versionId}{v.isDefaultVersion ? ' (default)' : ''}
						</button>
					{/each}
				</div>
				{#if activeVersionId && !versions.find((v) => v.versionId === activeVersionId)?.isDefaultVersion}
					<Button
						variant="outline"
						size="xs"
						onclick={() => activeVersionId && setDefault(activeVersionId)}
					>
						Set {activeVersionId} as default
					</Button>
				{/if}
			{/if}
		</div>
		<div class="mt-4">
			<PolicyEditor bind:value={policyDoc} id="policy-doc" label="Policy document" rows={18} />
			<div class="mt-2 flex justify-end">
				<Button size="sm" onclick={saveAsNewVersion} disabled={saving || !policyDoc}>
					{saving ? 'Saving...' : 'Save as new version'}
				</Button>
			</div>
		</div>
	{/if}
</EntityDetailSheet>
