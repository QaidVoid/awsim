<script lang="ts">
	import { onMount } from 'svelte';
	import { listCustomModels, type CustomModel } from '$lib/api/bedrock';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import LayersIcon from '@lucide/svelte/icons/layers';
	import { toast } from 'svelte-sonner';

	interface Props {
		onSelect: (m: CustomModel) => void;
	}

	let { onSelect }: Props = $props();

	let rows = $state<CustomModel[]>([]);
	let loading = $state(true);

	onMount(load);

	async function load() {
		loading = true;
		try {
			rows = await listCustomModels();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load custom models');
		} finally {
			loading = false;
		}
	}

	function fmtDate(s: string | null): string {
		if (!s) return '—';
		try {
			return new Date(s).toLocaleString();
		} catch {
			return s;
		}
	}
</script>

{#snippet baseCell(row: CustomModel)}
	<span class="text-xs">{row.baseModelName}</span>
{/snippet}

{#snippet typeCell(row: CustomModel)}
	{#if row.customizationType}
		<Badge variant="outline" class="text-[10px]">{row.customizationType}</Badge>
	{:else}
		<span class="text-[10px] text-muted-foreground">—</span>
	{/if}
{/snippet}

{#snippet createdCell(row: CustomModel)}
	<span class="text-[10px] text-muted-foreground">{fmtDate(row.creationTime)}</span>
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="text-xs text-muted-foreground">
			{rows.length} custom model{rows.length === 1 ? '' : 's'}
		</div>
		<Button variant="outline" size="sm" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'modelName', label: 'Name' },
			{ key: 'baseModelName', label: 'Base model', cell: baseCell },
			{ key: 'customizationType', label: 'Type', cell: typeCell },
			{ key: 'creationTime', label: 'Created', cell: createdCell },
		]}
		rowKey={(r) => r.modelArn}
		onRowClick={onSelect}
	>
		{#snippet empty()}
			<EmptyState
				icon={LayersIcon}
				title="No custom models"
				description="Fine-tune foundation models on your own data to create custom models."
			/>
		{/snippet}
	</DataTable>
</div>
