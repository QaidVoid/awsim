<script lang="ts">
	import { onMount } from 'svelte';
	import { listKeys, type Key } from '$lib/api/kms';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import KeyIcon from '@lucide/svelte/icons/key';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import KeyDetailSheet from './key-detail-sheet.svelte';

	let keys = $state<Key[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let selected = $state<Key | null>(null);
	let detailOpen = $state(false);

	const filtered = $derived(
		filter.trim() ? keys.filter((k) => k.keyId.includes(filter.trim())) : keys
	);

	async function load() {
		loading = true;
		try {
			keys = await listKeys();
		} finally {
			loading = false;
		}
	}

	function open(k: Key) {
		selected = k;
		detailOpen = true;
	}

	onMount(load);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input type="search" placeholder="Filter keys..." bind:value={filter} class="h-8 max-w-xs" />
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {keys.length}</Badge>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
	</div>
	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
			columns={[
				{ key: 'keyId', label: 'Key ID', width: '40%', mono: true },
				{ key: 'keyArn', label: 'ARN', mono: true }
			]}
			rowKey={(r: Key) => r.keyId}
			onRowClick={open}
		>
			{#snippet empty()}
				<EmptyState
					icon={KeyIcon}
					title="No KMS keys"
					description="Create one with: aws kms create-key --description 'My key'"
				/>
			{/snippet}
		</DataTable>
	</div>
</div>

<KeyDetailSheet
	k={selected}
	bind:open={detailOpen}
	onOpenChange={(v) => {
		detailOpen = v;
		if (!v) selected = null;
	}}
/>
