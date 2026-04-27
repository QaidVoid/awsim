<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import KeyIcon from '@lucide/svelte/icons/key';
	import { toast } from 'svelte-sonner';
	import { listPublicKeys, type PublicKey } from '$lib/api/cloudfront';

	let keys = $state<PublicKey[]>([]);
	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			keys = await listPublicKeys();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load public keys');
			keys = [];
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		void load();
	});
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Public keys
			<span class="ml-1 font-normal text-muted-foreground">({keys.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		rows={keys}
		{loading}
		rowKey={(k) => k.id}
		columns={[
			{ key: 'id', label: 'ID', mono: true, width: '180px' },
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'comment', label: 'Comment' },
			{ key: 'createdTime', label: 'Created', width: '200px' },
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={KeyIcon}
				title="No public keys"
				description="Upload a public key to verify signed URLs and cookies."
			/>
		{/snippet}
	</DataTable>
</div>
