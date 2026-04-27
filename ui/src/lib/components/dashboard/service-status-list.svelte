<script lang="ts">
	/**
	 * Compact list of registered services with a per-service activity
	 * health-dot, recent request count and on-disk size.
	 *
	 * Sorted by recent activity descending so busy services bubble up.
	 * Clicking a row navigates to the service's dedicated page.
	 */
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { Badge } from '$lib/components/ui/badge';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import Boxes from '@lucide/svelte/icons/boxes';
	import { goto } from '$app/navigation';
	import { dashboardState } from '$lib/dashboard-state.svelte';
	import { SERVICES, type Service } from '$lib/services-catalog';
	import { bytesHuman } from '$lib/format';
	import type { StoragePayload } from '$lib/events';

	interface Props {
		storage: StoragePayload | null;
	}

	let { storage }: Props = $props();

	// Tick clock so health bands re-evaluate even without new events.
	let now = $state(Date.now() / 1000);
	$effect(() => {
		const id = setInterval(() => (now = Date.now() / 1000), 5000);
		return () => clearInterval(id);
	});

	type Health = 'live' | 'warm' | 'idle';

	interface Row {
		svc: Service;
		recent: number;
		lastSeen: number | null;
		sizeBytes: number;
		health: Health;
	}

	const sizeMap = $derived.by(() => {
		const map = new Map<string, number>();
		for (const s of storage?.services ?? []) map.set(s.name.toLowerCase(), s.size_bytes);
		return map;
	});

	const rows = $derived.by<Row[]>(() => {
		const events = dashboardState.events;
		const fiveMin = now - 5 * 60;
		const oneHour = now - 3600;
		const counts = new Map<string, { count: number; lastSeen: number }>();
		for (const e of events) {
			const key = e.service.toLowerCase();
			const prev = counts.get(key);
			if (!prev) counts.set(key, { count: 1, lastSeen: e.ts });
			else {
				prev.count++;
				if (e.ts > prev.lastSeen) prev.lastSeen = e.ts;
			}
		}
		return SERVICES.map((svc) => {
			const c = counts.get(svc.id);
			const lastSeen = c?.lastSeen ?? null;
			let health: Health = 'idle';
			if (lastSeen !== null) {
				if (lastSeen >= fiveMin) health = 'live';
				else if (lastSeen >= oneHour) health = 'warm';
			}
			return {
				svc,
				recent: c?.count ?? 0,
				lastSeen,
				sizeBytes: sizeMap.get(svc.id) ?? 0,
				health,
			};
		}).sort((a, b) => {
			if (b.recent !== a.recent) return b.recent - a.recent;
			return b.sizeBytes - a.sizeBytes;
		});
	});

	function healthClass(h: Health): string {
		if (h === 'live') return 'bg-emerald-400 shadow-[0_0_6px_var(--color-emerald-400)]';
		if (h === 'warm') return 'bg-amber-400';
		return 'bg-muted-foreground/40';
	}
</script>

<Card class="h-full gap-0 p-0">
	<CardHeader class="border-b border-border px-4 py-3">
		<CardTitle class="flex items-center gap-2 text-sm font-semibold">
			<Boxes class="size-4 text-muted-foreground" />
			Services
			<span class="ml-auto text-[10px] font-normal text-muted-foreground">
				{rows.filter((r) => r.health !== 'idle').length} / {rows.length} active
			</span>
		</CardTitle>
	</CardHeader>
	<CardContent class="p-0">
		<ScrollArea class="h-[480px]">
			{#if rows.every((r) => r.recent === 0 && r.sizeBytes === 0)}
				<p class="px-4 pt-3 text-[11px] text-muted-foreground">
					No activity yet — list shows the full catalog ranked by future requests.
				</p>
			{/if}
			<ul class="divide-y divide-border/40">
				{#each rows as row (row.svc.id)}
					<li>
						<button
							type="button"
							onclick={() => goto(row.svc.href)}
							class="flex w-full items-center gap-3 px-4 py-2 text-left text-xs transition-colors hover:bg-muted/40"
						>
							<span class={`size-2 shrink-0 rounded-full ${healthClass(row.health)}`}></span>
							<row.svc.icon class="size-3.5 shrink-0 text-muted-foreground" />
							<span class="flex-1 truncate text-sm font-medium">{row.svc.name}</span>
							{#if row.recent > 0}
								<Badge variant="outline" class="font-mono text-[10px]">
									{row.recent} req
								</Badge>
							{/if}
							{#if row.sizeBytes > 0}
								<span class="font-mono text-[10px] text-muted-foreground">
									{bytesHuman(row.sizeBytes)}
								</span>
							{/if}
						</button>
					</li>
				{/each}
			</ul>
		</ScrollArea>
	</CardContent>
</Card>
