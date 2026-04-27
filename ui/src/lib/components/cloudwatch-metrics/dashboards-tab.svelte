<script lang="ts">
	/**
	 * Dashboards tab — list of CloudWatch dashboards. Clicking a row
	 * fetches its body JSON and shows it in a side panel.
	 */
	import { onMount } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import {
		listDashboards,
		getDashboard,
		type DashboardSummary,
		type DashboardDetail,
	} from '$lib/api/cloudwatch-metrics';
	import { EmptyState } from '$lib/components/service';
	import LayoutDashboard from '@lucide/svelte/icons/layout-dashboard';
	import { toast } from 'svelte-sonner';

	let dashboards = $state<DashboardSummary[]>([]);
	let loading = $state(true);
	let selected = $state<DashboardDetail | null>(null);
	let detailLoading = $state(false);

	async function reload() {
		loading = true;
		try {
			const data = await listDashboards();
			dashboards = data.dashboards;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load dashboards');
		} finally {
			loading = false;
		}
	}

	async function open(name: string) {
		detailLoading = true;
		try {
			selected = await getDashboard(name);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load dashboard');
		} finally {
			detailLoading = false;
		}
	}

	function pretty(body: string): string {
		try {
			return JSON.stringify(JSON.parse(body), null, 2);
		} catch {
			return body;
		}
	}

	onMount(reload);
</script>

<div class="grid h-full min-h-0 grid-cols-[minmax(0,1fr)_minmax(0,1fr)] divide-x divide-border">
	<div class="min-h-0 overflow-auto">
		{#if !loading && dashboards.length === 0}
			<div class="p-6">
				<EmptyState
					icon={LayoutDashboard}
					title="No dashboards"
					description="Create a dashboard via the AWS CLI to see it here."
				/>
			</div>
		{:else}
			<ul class="divide-y divide-border/40">
				{#each dashboards as d (d.name)}
					<li>
						<button
							type="button"
							onclick={() => open(d.name)}
							class="flex w-full items-center justify-between gap-3 px-4 py-2 text-left hover:bg-muted/30"
							class:bg-muted-50={selected?.name === d.name}
						>
							<div class="min-w-0">
								<div class="truncate font-mono text-sm">{d.name}</div>
								{#if d.lastModified}
									<div class="text-[11px] text-muted-foreground">{d.lastModified}</div>
								{/if}
							</div>
							{#if d.size != null}
								<Badge variant="outline" class="text-[10px]">{d.size}b</Badge>
							{/if}
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	</div>

	<div class="flex min-h-0 flex-col">
		<header class="flex shrink-0 items-center justify-between border-b border-border px-4 py-2">
			<h3 class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
				{selected?.name ?? 'Body'}
			</h3>
			{#if selected}
				<Button
					size="sm"
					variant="ghost"
					class="h-7 px-2 text-xs"
					onclick={() => (selected = null)}
				>
					Close
				</Button>
			{/if}
		</header>
		<div class="min-h-0 flex-1 overflow-auto bg-muted/10 p-3">
			{#if detailLoading}
				<p class="text-xs text-muted-foreground">Loading…</p>
			{:else if !selected}
				<p class="text-xs text-muted-foreground">Pick a dashboard on the left.</p>
			{:else}
				<pre class="whitespace-pre-wrap break-words font-mono text-[11px] text-foreground/90">{pretty(selected.body)}</pre>
			{/if}
		</div>
	</div>
</div>
