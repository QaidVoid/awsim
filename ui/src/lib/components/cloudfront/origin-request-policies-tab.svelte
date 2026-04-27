<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import RouteIcon from '@lucide/svelte/icons/route';
	import { toast } from 'svelte-sonner';
	import {
		listOriginRequestPolicies,
		type OriginRequestPolicy,
	} from '$lib/api/cloudfront';

	let policies = $state<OriginRequestPolicy[]>([]);
	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			policies = await listOriginRequestPolicies();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load origin request policies');
			policies = [];
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
			Origin request policies
			<span class="ml-1 font-normal text-muted-foreground">({policies.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		rows={policies}
		{loading}
		rowKey={(p) => p.id}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'type', label: 'Type', width: '120px', cell: typeCell },
			{ key: 'comment', label: 'Comment' },
			{ key: 'lastModifiedTime', label: 'Last modified', width: '200px' },
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={RouteIcon}
				title="No origin request policies"
				description="Origin request policies control what's forwarded to your origin."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet typeCell(p: OriginRequestPolicy)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] uppercase">{p.type || 'custom'}</Badge>
{/snippet}
