<script lang="ts">
	import {
		listRuleGroups,
		getRuleGroup,
		type WafScope,
		type RuleGroup,
		type RuleGroupDetail
	} from '$lib/api/waf';
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
	import Layers from '@lucide/svelte/icons/layers';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import { toast } from 'svelte-sonner';

	interface Props {
		scope: WafScope;
	}

	let { scope }: Props = $props();

	let groups = $state<RuleGroup[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let detail = $state<RuleGroupDetail | null>(null);
	let detailOpen = $state(false);
	let detailLoading = $state(false);

	const filtered = $derived(
		filter.trim()
			? groups.filter((g) => g.name.toLowerCase().includes(filter.trim().toLowerCase()))
			: groups
	);

	async function load() {
		loading = true;
		try {
			groups = await listRuleGroups(scope);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load rule groups');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		void scope;
		load();
	});

	async function open(g: RuleGroup) {
		detailOpen = true;
		detail = null;
		detailLoading = true;
		try {
			detail = await getRuleGroup(g.name, g.id, scope);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load detail');
		} finally {
			detailLoading = false;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input
			type="search"
			placeholder="Filter rule groups..."
			bind:value={filter}
			class="h-8 max-w-xs"
		/>
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {groups.length}</Badge>
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
			rowKey={(r: RuleGroup) => r.arn || r.id}
			onRowClick={open}
		>
			{#snippet empty()}
				<EmptyState
					icon={Layers}
					title="No custom rule groups in {scope} scope"
					description="Rule groups let you define reusable bundles of WAF rules."
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
					{#if detail.capacity !== undefined}
						<dt class="text-muted-foreground">Capacity</dt>
						<dd class="col-span-2">{detail.capacity}</dd>
					{/if}
					{#if detail.description}
						<dt class="text-muted-foreground">Description</dt>
						<dd class="col-span-2">{detail.description}</dd>
					{/if}
				</dl>
				<h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
					Rules
				</h3>
				{#if !detail.rules?.length}
					<p class="text-xs text-muted-foreground">No rules.</p>
				{:else}
					<ul class="space-y-1">
						{#each detail.rules as r (r.name + r.priority)}
							<li
								class="flex items-center justify-between rounded border border-border/60 px-3 py-2 text-sm"
							>
								<div>
									<div class="font-medium">{r.name}</div>
									<div class="text-xs text-muted-foreground">priority {r.priority}</div>
								</div>
								<Badge variant="outline">{r.action}</Badge>
							</li>
						{/each}
					</ul>
				{/if}
			{/if}
		</div>
	</SheetContent>
</Sheet>
