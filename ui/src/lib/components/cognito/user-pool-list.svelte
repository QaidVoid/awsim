<script lang="ts">
	import { listUserPools, type UserPool } from '$lib/api/cognito';
	import { onMount } from 'svelte';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import Users from '@lucide/svelte/icons/users';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';

	interface Props {
		onSelect: (pool: UserPool) => void;
	}

	let { onSelect }: Props = $props();

	let pools = $state<UserPool[]>([]);
	let loading = $state(false);
	let filter = $state('');

	const filtered = $derived(
		filter.trim()
			? pools.filter(
					(p) =>
						p.name.toLowerCase().includes(filter.trim().toLowerCase()) ||
						p.id.includes(filter.trim())
				)
			: pools
	);

	async function load() {
		loading = true;
		try {
			pools = await listUserPools();
		} finally {
			loading = false;
		}
	}

	onMount(load);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input
			type="search"
			placeholder="Filter user pools..."
			bind:value={filter}
			class="h-8 max-w-xs"
		/>
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {pools.length}</Badge>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
	</div>
	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
			columns={[
				{ key: 'name', label: 'Name', width: '30%' },
				{ key: 'id', label: 'Pool ID', mono: true, width: '35%' },
				{ key: 'status', label: 'Status', width: '15%' },
				{ key: 'creationDate', label: 'Created', width: '20%' }
			]}
			rowKey={(r: UserPool) => r.id}
			onRowClick={onSelect}
		>
			{#snippet empty()}
				<EmptyState
					icon={Users}
					title="No user pools"
					description="Create one with: aws cognito-idp create-user-pool --pool-name MyPool"
				/>
			{/snippet}
		</DataTable>
	</div>
</div>
