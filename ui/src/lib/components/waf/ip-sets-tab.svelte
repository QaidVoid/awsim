<script lang="ts">
	import { listIpSets, getIpSet, type WafScope, type IpSet, type IpSetDetail } from '$lib/api/waf';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import Network from '@lucide/svelte/icons/network';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import { toast } from 'svelte-sonner';

	interface Props {
		scope: WafScope;
	}

	let { scope }: Props = $props();

	let sets = $state<IpSet[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let detail = $state<IpSetDetail | null>(null);
	let detailOpen = $state(false);
	let detailLoading = $state(false);

	const filtered = $derived(
		filter.trim()
			? sets.filter((s) => s.name.toLowerCase().includes(filter.trim().toLowerCase()))
			: sets
	);

	async function load() {
		loading = true;
		try {
			sets = await listIpSets(scope);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load IP sets');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		void scope;
		load();
	});

	async function open(s: IpSet) {
		detailOpen = true;
		detail = null;
		detailLoading = true;
		try {
			detail = await getIpSet(s.name, s.id, scope);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load detail');
		} finally {
			detailLoading = false;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input type="search" placeholder="Filter IP sets..." bind:value={filter} class="h-8 max-w-xs" />
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {sets.length}</Badge>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
	</div>
	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
			{loading}
			columns={[
				{ key: 'name', label: 'Name', width: '25%' },
				{ key: 'arn', label: 'ARN', mono: true },
				{ key: 'description', label: 'Description', width: '25%' }
			]}
			rowKey={(r: IpSet) => r.arn || r.id}
			onRowClick={open}
		>
			{#snippet empty()}
				<EmptyState
					icon={Network}
					title="No IP sets in {scope} scope"
					description="IP sets are reusable lists of CIDR ranges referenced by rules."
				/>
			{/snippet}
		</DataTable>
	</div>
</div>

<Sheet bind:open={detailOpen} onOpenChange={(v) => (detailOpen = v)}>
	<SheetContent side="right" class="w-full max-w-2xl overflow-y-auto sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle>{detail?.name ?? ''}</SheetTitle>
			<SheetDescription class="truncate font-mono text-xs">{detail?.arn ?? ''}</SheetDescription>
		</SheetHeader>
		<div class="px-6 pb-6">
			{#if detailLoading}
				<p class="text-xs text-muted-foreground">Loading...</p>
			{:else if detail}
				<dl class="grid grid-cols-3 gap-x-4 gap-y-2 py-4 text-sm">
					<dt class="text-muted-foreground">Address version</dt>
					<dd class="col-span-2">{detail.ipAddressVersion ?? '—'}</dd>
					{#if detail.description}
						<dt class="text-muted-foreground">Description</dt>
						<dd class="col-span-2">{detail.description}</dd>
					{/if}
				</dl>
				<h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
					Addresses ({detail.addresses.length})
				</h3>
				{#if !detail.addresses.length}
					<p class="text-xs text-muted-foreground">No addresses configured.</p>
				{:else}
					<ul class="grid grid-cols-2 gap-1">
						{#each detail.addresses as a (a)}
							<li class="rounded border border-border/60 px-3 py-1.5 font-mono text-xs">{a}</li>
						{/each}
					</ul>
				{/if}
			{/if}
		</div>
	</SheetContent>
</Sheet>
