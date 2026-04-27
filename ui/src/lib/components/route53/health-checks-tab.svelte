<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import HeartPulseIcon from '@lucide/svelte/icons/heart-pulse';
	import { toast } from 'svelte-sonner';
	import { listHealthChecks, type HealthCheck } from '$lib/api/route53';

	let checks = $state<HealthCheck[]>([]);
	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			checks = await listHealthChecks();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load health checks');
			checks = [];
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
			Health checks
			<span class="ml-1 font-normal text-muted-foreground">({checks.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		rows={checks}
		{loading}
		rowKey={(c) => c.id}
		columns={[
			{ key: 'id', label: 'ID', mono: true, width: '180px' },
			{ key: 'type', label: 'Type', width: '110px', cell: typeCell },
			{ key: 'target', label: 'Target', cell: targetCell },
			{ key: 'port', label: 'Port', width: '90px', align: 'right' },
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={HeartPulseIcon}
				title="No health checks"
				description="Health checks monitor endpoints and can fail over DNS records."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet typeCell(c: HealthCheck)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">{c.type || '—'}</Badge>
{/snippet}

{#snippet targetCell(c: HealthCheck)}
	<span class="font-mono text-[11px]">
		{c.fullyQualifiedDomainName || c.ipAddress || '—'}{c.resourcePath ?? ''}
	</span>
{/snippet}
