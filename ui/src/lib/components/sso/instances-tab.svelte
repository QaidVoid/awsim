<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import BuildingIcon from '@lucide/svelte/icons/building';
	import { toast } from 'svelte-sonner';
	import { listInstances, type Instance } from '$lib/api/sso-admin';

	interface Props {
		instances: Instance[];
		onLoaded: (instances: Instance[]) => void;
	}

	let { instances, onLoaded }: Props = $props();

	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			const data = await listInstances();
			onLoaded(data);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load instances');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		if (instances.length === 0) load();
	});
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">Identity Center instances ({instances.length})</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		rows={instances}
		{loading}
		rowKey={(i) => i.instanceArn}
		columns={[
			{ key: 'instanceArn', label: 'Instance ARN', mono: true },
			{ key: 'identityStoreId', label: 'Identity store', mono: true },
			{ key: 'name', label: 'Name' },
			{ key: 'status', label: 'Status' }
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={BuildingIcon}
				title="No instances"
				description="No IAM Identity Center instances are configured."
			/>
		{/snippet}
	</DataTable>

	{#if instances.length > 0}
		<div class="mt-2 flex flex-wrap gap-2">
			{#each instances as inst (inst.instanceArn)}
				<Badge variant="outline" class="font-mono text-[10px]">
					{inst.instanceArn}
				</Badge>
			{/each}
		</div>
	{/if}
</div>
