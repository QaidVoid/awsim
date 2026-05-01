<script lang="ts">
	import { onMount } from 'svelte';
	import { listRoles, getRole, deleteRole, type IamRole } from '$lib/api/iam';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import EntityDetailSheet from './entity-detail-sheet.svelte';
	import CreateEntityDialog from './create-entity-dialog.svelte';
	import PolicyEditor from './policy-editor.svelte';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { toast } from 'svelte-sonner';

	let roles = $state<IamRole[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let selected = $state<IamRole | null>(null);
	let trustDoc = $state('');
	let detailLoading = $state(false);
	let createOpen = $state(false);
	let deleting = $state(false);

	const filtered = $derived(
		filter.trim()
			? roles.filter((r) => r.roleName.toLowerCase().includes(filter.trim().toLowerCase()))
			: roles
	);

	async function load() {
		loading = true;
		try {
			roles = await listRoles();
		} finally {
			loading = false;
		}
	}

	async function openDetail(r: IamRole) {
		selected = r;
		trustDoc = '';
		detailLoading = true;
		try {
			const detail = await getRole(r.roleName);
			selected = detail;
			if (detail.assumeRolePolicyDocument) {
				try {
					trustDoc = JSON.stringify(JSON.parse(detail.assumeRolePolicyDocument), null, 2);
				} catch {
					trustDoc = detail.assumeRolePolicyDocument;
				}
			}
		} finally {
			detailLoading = false;
		}
	}

	async function handleDelete(r: IamRole) {
		if (!confirm(`Delete role "${r.roleName}"? This cannot be undone.`)) return;
		deleting = true;
		try {
			await deleteRole(r.roleName);
			toast.success(`Deleted ${r.roleName}`);
			selected = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleting = false;
		}
	}

	onMount(load);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input type="search" placeholder="Filter roles..." bind:value={filter} class="h-8 max-w-xs" />
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {roles.length}</Badge>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<Plus class="size-3.5" />
			<span class="ml-1">New role</span>
		</Button>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
	</div>

	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
			{loading}
			columns={[
				{ key: 'roleName', label: 'Role name', width: '25%' },
				{ key: 'arn', label: 'ARN', mono: true },
				{ key: 'description', label: 'Description', width: '30%' }
			]}
			rowKey={(r: IamRole) => r.arn || r.roleName}
			onRowClick={openDetail}
		>
			{#snippet empty()}
				<EmptyState
					icon={ShieldCheck}
					title="No IAM roles"
					description="Roles let services assume permissions on your behalf."
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
	title={selected?.roleName ?? ''}
	subtitle={selected?.arn}
>
	{#if selected}
		<dl class="grid grid-cols-3 gap-x-4 gap-y-2 py-4 text-sm">
			<dt class="text-muted-foreground">Role ID</dt>
			<dd class="col-span-2 font-mono text-xs">{selected.roleId}</dd>
			{#if selected.description}
				<dt class="text-muted-foreground">Description</dt>
				<dd class="col-span-2">{selected.description}</dd>
			{/if}
			<dt class="text-muted-foreground">ARN</dt>
			<dd class="col-span-2 break-all font-mono text-xs">{selected.arn}</dd>
		</dl>
		<div class="mt-4">
			<PolicyEditor
				bind:value={trustDoc}
				id="role-trust-policy"
				label="Trust policy"
				readonly
				rows={16}
			/>
			{#if detailLoading}
				<p class="mt-2 text-xs text-muted-foreground">Loading trust policy...</p>
			{/if}
		</div>
		<div class="flex justify-end pt-4">
			<Button
				variant="destructive"
				size="sm"
				disabled={deleting}
				onclick={() => selected && handleDelete(selected)}
			>
				<Trash2 class="size-4" />
				<span class="ml-1">Delete role</span>
			</Button>
		</div>
	{/if}
</EntityDetailSheet>

<CreateEntityDialog
	bind:open={createOpen}
	kind="role"
	onOpenChange={(v) => (createOpen = v)}
	onCreated={load}
/>
