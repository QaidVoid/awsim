<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listGuardrails,
		deleteGuardrail,
		type Guardrail
	} from '$lib/api/bedrock';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { toast } from 'svelte-sonner';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import ShieldIcon from '@lucide/svelte/icons/shield';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';

	interface Props {
		onCreate: () => void;
		onSelect: (g: Guardrail) => void;
	}

	let { onCreate, onSelect }: Props = $props();

	let rows = $state<Guardrail[]>([]);
	let loading = $state(true);

	onMount(load);

	export async function load() {
		loading = true;
		try {
			rows = await listGuardrails();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load guardrails');
		} finally {
			loading = false;
		}
	}

	async function handleDelete(g: Guardrail) {
		if (!confirm(`Delete guardrail ${g.name}?`)) return;
		try {
			await deleteGuardrail(g.guardrailId);
			toast.success(`Guardrail ${g.name} deleted.`);
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}

	function statusVariant(s: string): 'secondary' | 'destructive' | 'outline' {
		if (s === 'READY') return 'secondary';
		if (s === 'FAILED') return 'destructive';
		return 'outline';
	}
</script>

{#snippet statusCell(row: Guardrail)}
	<Badge variant={statusVariant(row.status)} class="text-[10px]">{row.status || '—'}</Badge>
{/snippet}

{#snippet versionCell(row: Guardrail)}
	<span class="font-mono text-xs">{row.version}</span>
{/snippet}

{#snippet actionsCell(row: Guardrail)}
	<div class="flex justify-end gap-1">
		<Button size="xs" variant="ghost" onclick={() => onSelect(row)}>View</Button>
		<Button size="xs" variant="ghost" onclick={() => handleDelete(row)}>
			<Trash2Icon class="size-3" />
		</Button>
	</div>
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="text-xs text-muted-foreground">
			{rows.length} guardrail{rows.length === 1 ? '' : 's'}
		</div>
		<div class="flex items-center gap-2">
			<Button variant="outline" size="sm" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={onCreate}>
				<PlusIcon />
				New guardrail
			</Button>
		</div>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name' },
			{ key: 'guardrailId', label: 'ID', mono: true },
			{ key: 'status', label: 'Status', cell: statusCell },
			{ key: 'version', label: 'Version', cell: versionCell },
			{ key: 'actions', label: '', align: 'right', width: '160px', cell: actionsCell }
		]}
		rowKey={(r) => r.guardrailId}
	>
		{#snippet empty()}
			<EmptyState
				icon={ShieldIcon}
				title="No guardrails"
				description="Define content policies to filter unsafe model inputs and outputs."
			>
				{#snippet action()}
					<Button onclick={onCreate}>
						<PlusIcon />
						Create guardrail
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>
