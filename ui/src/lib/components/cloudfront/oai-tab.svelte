<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import { toast } from 'svelte-sonner';
	import {
		listOriginAccessIdentities,
		type OriginAccessIdentity,
	} from '$lib/api/cloudfront';

	let oais = $state<OriginAccessIdentity[]>([]);
	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			oais = await listOriginAccessIdentities();
		} catch (e) {
			toast.error(
				e instanceof Error ? e.message : 'Failed to load origin access identities',
			);
			oais = [];
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
			Origin access identities
			<span class="ml-1 font-normal text-muted-foreground">({oais.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		rows={oais}
		{loading}
		rowKey={(o) => o.id}
		columns={[
			{ key: 'id', label: 'ID', mono: true, width: '180px' },
			{ key: 'comment', label: 'Comment' },
			{ key: 's3CanonicalUserId', label: 'S3 canonical user', mono: true },
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={KeyRoundIcon}
				title="No origin access identities"
				description="OAIs grant CloudFront access to private S3 origin content."
			/>
		{/snippet}
	</DataTable>
</div>
