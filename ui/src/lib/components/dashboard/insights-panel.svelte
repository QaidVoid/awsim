<script lang="ts">
	/**
	 * Insights panel — derived bullets that summarise what's happening
	 * across the local emulator. Recomputed every 30s from the live
	 * SSE buffer plus the polled storage and config payloads.
	 *
	 * Each insight is dismissible per session (not persisted). New
	 * insights with the same key reappear after the regen tick.
	 */
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import Lightbulb from '@lucide/svelte/icons/lightbulb';
	import X from '@lucide/svelte/icons/x';
	import Flame from '@lucide/svelte/icons/flame';
	import HardDrive from '@lucide/svelte/icons/hard-drive';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import ShieldOff from '@lucide/svelte/icons/shield-off';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Package from '@lucide/svelte/icons/package';
	import { dashboardState } from '$lib/dashboard-state.svelte';
	import { bytesHuman } from '$lib/format';
	import type { StoragePayload } from '$lib/events';
	import type { Component } from 'svelte';

	interface Props {
		storage: StoragePayload | null;
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		config: Record<string, any> | null;
	}

	let { storage, config }: Props = $props();

	interface Insight {
		key: string;
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		icon: Component<any>;
		text: string;
		tone: 'default' | 'warning' | 'destructive';
	}

	// Tick a regen counter every 30s — used as a $derived dependency so
	// the insight list refreshes on a coarse cadence rather than every
	// single new event.
	let tick = $state(0);
	$effect(() => {
		const id = setInterval(() => (tick++), 30_000);
		return () => clearInterval(id);
	});

	let dismissed = $state(new Set<string>());

	const insights = $derived.by<Insight[]>(() => {
		void tick; // re-trigger every regen tick
		const out: Insight[] = [];
		const events = dashboardState.events;
		const now = Date.now() / 1000;
		const fiveMin = now - 5 * 60;
		const oneHour = now - 3600;

		// Top service in the last 5 minutes.
		const recentByService = new Map<string, number>();
		let recentCount = 0;
		for (const e of events) {
			if (e.ts < fiveMin) break;
			recentByService.set(e.service, (recentByService.get(e.service) ?? 0) + 1);
			recentCount++;
		}
		const top = [...recentByService.entries()].sort((a, b) => b[1] - a[1])[0];
		if (recentCount > 0) {
			out.push({
				key: 'top-5min',
				icon: Flame,
				text: top
					? `${recentCount} requests in the last 5 minutes — top service ${top[0]} (${top[1]})`
					: `${recentCount} requests in the last 5 minutes`,
				tone: 'default',
			});
		}

		// Disk usage summary.
		if (storage?.data_dir && storage.services?.length) {
			const total = storage.total_size_bytes ?? storage.services.reduce((a, s) => a + s.size_bytes, 0);
			const nonzero = storage.services.filter((s) => s.size_bytes > 0).length;
			out.push({
				key: 'disk',
				icon: HardDrive,
				text: `${bytesHuman(total)} on disk across ${nonzero} service${nonzero === 1 ? '' : 's'}`,
				tone: 'default',
			});
		} else if (storage && storage.data_dir === null) {
			out.push({
				key: 'disk-off',
				icon: HardDrive,
				text: 'Persistence is off — state is in-memory only',
				tone: 'warning',
			});
		}

		// IAM enforcement.
		if (config) {
			const iamOn =
				config.iamEnforcement === true ||
				config.iam_enforcement === true ||
				config.enforceIam === true;
			out.push({
				key: 'iam',
				icon: iamOn ? ShieldCheck : ShieldOff,
				text: `IAM enforcement: ${iamOn ? 'on' : 'off'}`,
				tone: iamOn ? 'default' : 'warning',
			});
		}

		// 5xx rate in the last hour.
		let serverErrors = 0;
		for (const e of events) {
			if (e.ts < oneHour) break;
			if (e.status_code >= 500) serverErrors++;
		}
		if (serverErrors > 0) {
			out.push({
				key: '5xx',
				icon: AlertTriangle,
				text: `${serverErrors} request${serverErrors === 1 ? '' : 's'} returned 5xx in the last hour`,
				tone: 'destructive',
			});
		}

		// Storage breakdown by blob count for headline services.
		if (storage?.services?.length) {
			const headline = ['s3', 'lambda', 'dynamodb']
				.map((id) => storage.services.find((s) => s.name.toLowerCase() === id))
				.filter((s): s is NonNullable<typeof s> => Boolean(s) && s!.blob_count > 0);
			if (headline.length > 0) {
				const parts = headline.map((s) => `${s.blob_count} ${s.name} blobs`);
				out.push({
					key: 'blobs',
					icon: Package,
					text: parts.join(' · '),
					tone: 'default',
				});
			}
		}

		return out.filter((i) => !dismissed.has(i.key));
	});

	function dismiss(key: string) {
		const next = new Set(dismissed);
		next.add(key);
		dismissed = next;
	}

	function toneClass(tone: Insight['tone']): string {
		if (tone === 'destructive') return 'border-rose-500/30 bg-rose-500/5 text-rose-300';
		if (tone === 'warning') return 'border-amber-500/30 bg-amber-500/5 text-amber-200';
		return 'border-border bg-muted/40 text-foreground/90';
	}
</script>

<Card class="gap-0 p-0">
	<CardHeader class="border-b border-border px-4 py-3">
		<CardTitle class="flex items-center gap-2 text-sm font-semibold">
			<Lightbulb class="size-4 text-muted-foreground" />
			Insights
		</CardTitle>
	</CardHeader>
	<CardContent class="p-4">
		{#if insights.length === 0}
			<p class="text-xs text-muted-foreground">
				No insights yet — start hitting the emulator to populate this panel.
			</p>
		{:else}
			<div class="flex flex-wrap gap-2">
				{#each insights as ins (ins.key)}
					<div
						class={`group inline-flex items-center gap-2 rounded-full border px-3 py-1.5 text-xs ${toneClass(ins.tone)}`}
					>
						<ins.icon class="size-3.5 shrink-0" />
						<span>{ins.text}</span>
						<Button
							size="sm"
							variant="ghost"
							class="h-5 w-5 p-0 opacity-50 hover:opacity-100"
							onclick={() => dismiss(ins.key)}
							title="Dismiss"
						>
							<X class="size-3" />
						</Button>
					</div>
				{/each}
			</div>
		{/if}
	</CardContent>
</Card>
