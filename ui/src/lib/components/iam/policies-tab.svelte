<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { route } from '$lib/url';
	import { listPolicies, type IamPolicy } from '$lib/api/iam';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import CreateEntityDialog from './create-entity-dialog.svelte';
	import FileBadge from '@lucide/svelte/icons/file-badge';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';

	let policies = $state<IamPolicy[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let createOpen = $state(false);

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

	function openDetail(p: IamPolicy) {
		goto(route(`/iam/policies/${encodeURIComponent(p.arn)}`));
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
		<Button size="sm" onclick={() => (createOpen = true)}>
			<Plus class="size-3.5" />
			<span class="ml-1">New policy</span>
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
					description="Managed policies are reusable permission documents you can attach to users, groups, and roles to grant access."
				>
					{#snippet action()}
						<Button size="sm" onclick={() => (createOpen = true)}>
							<Plus class="size-3.5" />
							Create policy
						</Button>
					{/snippet}
				</EmptyState>
			{/snippet}
		</DataTable>
	</div>
</div>

<CreateEntityDialog
	bind:open={createOpen}
	kind="policy"
	onOpenChange={(v) => (createOpen = v)}
	onCreated={load}
/>
