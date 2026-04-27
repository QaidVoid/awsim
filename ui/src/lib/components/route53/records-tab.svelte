<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import GlobeIcon from '@lucide/svelte/icons/globe';
	import { toast } from 'svelte-sonner';
	import {
		listResourceRecordSets,
		changeResourceRecordSets,
		type ResourceRecordSet,
	} from '$lib/api/route53';

	interface Props {
		hostedZoneId: string;
		zoneName: string;
		onCreate: () => void;
		refreshKey?: number;
	}

	let { hostedZoneId, zoneName, onCreate, refreshKey = 0 }: Props = $props();

	let records = $state<ResourceRecordSet[]>([]);
	let loading = $state(false);

	$effect(() => {
		hostedZoneId;
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			records = await listResourceRecordSets(hostedZoneId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load records');
			records = [];
		} finally {
			loading = false;
		}
	}

	function isApex(name: string): boolean {
		const n = name.endsWith('.') ? name.slice(0, -1) : name;
		const z = zoneName.endsWith('.') ? zoneName.slice(0, -1) : zoneName;
		return n === z;
	}

	function deletable(r: ResourceRecordSet): boolean {
		return !(isApex(r.name) && (r.type === 'NS' || r.type === 'SOA'));
	}

	async function handleDelete(r: ResourceRecordSet) {
		if (!deletable(r)) {
			toast.error('Apex NS/SOA records cannot be deleted.');
			return;
		}
		try {
			await changeResourceRecordSets(hostedZoneId, [
				{
					action: 'DELETE',
					name: r.name,
					type: r.type,
					ttl: r.ttl,
					values: r.records.map((v) => v.value),
				},
			]);
			toast.success('Record deleted.');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete record');
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Records
			<span class="ml-1 font-normal text-muted-foreground">({records.length})</span>
		</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={onCreate}>
				<PlusIcon />
				New record
			</Button>
		</div>
	</div>

	<DataTable
		rows={records}
		{loading}
		rowKey={(r) => `${r.name}-${r.type}-${r.setIdentifier ?? ''}`}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'type', label: 'Type', width: '100px', cell: typeCell },
			{ key: 'ttl', label: 'TTL', width: '100px', align: 'right' },
			{ key: 'value', label: 'Value', cell: valueCell },
			{ key: 'actions', label: '', width: '60px', cell: actionsCell },
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={GlobeIcon}
				title="No records"
				description="Add A, AAAA, CNAME, MX, TXT, or other DNS records to route traffic."
			>
				{#snippet action()}
					<Button onclick={onCreate}>
						<PlusIcon />
						Create record
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

{#snippet typeCell(r: ResourceRecordSet)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">{r.type}</Badge>
{/snippet}

{#snippet valueCell(r: ResourceRecordSet)}
	<div class="flex flex-col gap-0.5">
		{#each r.records as v (v.value)}
			<span class="font-mono text-[11px] truncate">{v.value}</span>
		{/each}
		{#if r.aliasTarget}
			<span class="font-mono text-[11px] text-muted-foreground">
				ALIAS → {r.aliasTarget.dnsName}
			</span>
		{/if}
	</div>
{/snippet}

{#snippet actionsCell(r: ResourceRecordSet)}
	<Button
		size="xs"
		variant="ghost"
		class="text-destructive hover:text-destructive"
		onclick={() => handleDelete(r)}
		disabled={!deletable(r)}
		aria-label="Delete record"
	>
		<Trash2Icon />
	</Button>
{/snippet}
