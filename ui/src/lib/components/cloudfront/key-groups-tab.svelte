<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import UsersIcon from '@lucide/svelte/icons/users';
	import { toast } from 'svelte-sonner';
	import { listKeyGroups, type KeyGroup } from '$lib/api/cloudfront';

	let groups = $state<KeyGroup[]>([]);
	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			groups = await listKeyGroups();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load key groups');
			groups = [];
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
			Key groups
			<span class="ml-1 font-normal text-muted-foreground">({groups.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		rows={groups}
		{loading}
		rowKey={(g) => g.id}
		columns={[
			{ key: 'id', label: 'ID', mono: true, width: '180px' },
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'publicKeyIds', label: 'Public keys', cell: keysCell },
			{ key: 'lastModifiedTime', label: 'Last modified', width: '200px' },
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={UsersIcon}
				title="No key groups"
				description="Key groups bundle public keys used to validate signed URLs/cookies."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet keysCell(g: KeyGroup)}
	<div class="flex flex-wrap gap-1">
		{#each g.publicKeyIds as id (id)}
			<Badge variant="outline" class="h-4 px-1.5 text-[10px] font-mono">{id}</Badge>
		{:else}
			<span class="text-xs text-muted-foreground">—</span>
		{/each}
	</div>
{/snippet}
