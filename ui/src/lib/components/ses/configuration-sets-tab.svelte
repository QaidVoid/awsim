<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import SlidersIcon from '@lucide/svelte/icons/sliders-horizontal';
	import { toast } from 'svelte-sonner';
	import {
		listConfigurationSets,
		getConfigurationSet,
		type ConfigurationSet
	} from '$lib/api/ses';

	let sets = $state<ConfigurationSet[]>([]);
	let loading = $state(false);
	let detailFor = $state<string | null>(null);
	let detail = $state<ConfigurationSet | null>(null);

	async function load() {
		loading = true;
		try {
			sets = await listConfigurationSets();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load configuration sets');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
	});

	async function inspect(name: string) {
		detailFor = name;
		detail = null;
		try {
			detail = await getConfigurationSet(name);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load configuration set');
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">Configuration sets ({sets.length})</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="grid gap-4 lg:grid-cols-3">
		<div class="lg:col-span-2">
			<DataTable
				rows={sets}
				{loading}
				rowKey={(s) => s.name}
				onRowClick={(s) => inspect(s.name)}
				columns={[{ key: 'name', label: 'Name', mono: true }]}
			>
				{#snippet empty()}
					<EmptyState
						icon={SlidersIcon}
						title="No configuration sets"
						description="Configuration sets group sending settings: tracking, reputation metrics, IP pools."
					/>
				{/snippet}
			</DataTable>
		</div>

		<aside
			class="rounded-md border border-border bg-card/40 p-3 text-xs"
			aria-label="Configuration set detail"
		>
			<p class="mb-2 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
				Detail
			</p>
			{#if !detailFor}
				<p class="text-muted-foreground">Click a configuration set to inspect its settings.</p>
			{:else if detail === null}
				<div class="flex flex-col gap-2">
					<Skeleton class="h-3 w-2/3" />
					<Skeleton class="h-3 w-1/2" />
					<Skeleton class="h-3 w-3/4" />
				</div>
			{:else}
				<dl class="grid grid-cols-3 gap-x-2 gap-y-1.5">
					<dt class="text-muted-foreground">Name</dt>
					<dd class="col-span-2 break-all font-mono">{detail.name}</dd>
					<dt class="text-muted-foreground">Sending</dt>
					<dd class="col-span-2">
						{#if detail.sendingEnabled}
							<Badge variant="outline" class="h-4 px-1.5 text-[10px] text-green-500">
								enabled
							</Badge>
						{:else}
							<Badge
								variant="outline"
								class="h-4 px-1.5 text-[10px] text-muted-foreground"
							>
								disabled
							</Badge>
						{/if}
					</dd>
					<dt class="text-muted-foreground">Reputation</dt>
					<dd class="col-span-2">
						{detail.reputationOptions?.reputationMetricsEnabled ? 'metrics on' : 'metrics off'}
					</dd>
					<dt class="text-muted-foreground">Tracking</dt>
					<dd class="col-span-2 break-all font-mono">
						{detail.trackingOptions?.customRedirectDomain ?? '—'}
					</dd>
				</dl>
			{/if}
		</aside>
	</div>
</div>
