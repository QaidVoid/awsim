<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { route } from '$lib/url';
	import { listRoles, type IamRole } from '$lib/api/iam';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import CreateEntityDialog from './create-entity-dialog.svelte';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';

	let roles = $state<IamRole[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let createOpen = $state(false);

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

	function openDetail(r: IamRole) {
		goto(route(`/iam/roles/${encodeURIComponent(r.roleName)}`));
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

<CreateEntityDialog
	bind:open={createOpen}
	kind="role"
	onOpenChange={(v) => (createOpen = v)}
	onCreated={load}
/>
