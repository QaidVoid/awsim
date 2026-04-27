<script lang="ts">
	import { tailLogs, type LogEvent } from '$lib/api/lambda';
	import { Button } from '$lib/components/ui/button';
	import { EmptyState } from '$lib/components/service';
	import { toast } from 'svelte-sonner';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import ScrollText from '@lucide/svelte/icons/scroll-text';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		functionName: string;
	}

	let { functionName }: Props = $props();

	let events = $state<LogEvent[]>([]);
	let loading = $state(false);
	let autoRefresh = $state(false);
	let timer: ReturnType<typeof setInterval> | null = null;
	let lastFn = $state('');

	$effect(() => {
		if (functionName && functionName !== lastFn) {
			lastFn = functionName;
			void load();
		}
	});

	$effect(() => {
		if (autoRefresh && functionName) {
			timer = setInterval(() => {
				void load();
			}, 5000);
		} else if (timer) {
			clearInterval(timer);
			timer = null;
		}
		return () => {
			if (timer) {
				clearInterval(timer);
				timer = null;
			}
		};
	});

	async function load() {
		loading = true;
		try {
			const r = await tailLogs(functionName, 200);
			events = r.events;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to fetch logs');
		} finally {
			loading = false;
		}
	}

	function formatTs(ts: number): string {
		if (!ts) return '—';
		try {
			return new Date(ts).toLocaleTimeString();
		} catch {
			return String(ts);
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header
		class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2"
	>
		<div>
			<div class="text-sm font-medium">/aws/lambda/{functionName}</div>
			<div class="mt-0.5 text-[11px] text-muted-foreground">
				{events.length} events · most recent first
			</div>
		</div>
		<div class="flex items-center gap-2">
			<label class="flex items-center gap-1.5 text-xs text-muted-foreground">
				<input
					type="checkbox"
					bind:checked={autoRefresh}
					class="size-3.5 accent-primary"
				/>
				Auto-refresh
			</label>
			<Button type="button" variant="outline" size="sm" onclick={load} disabled={loading}>
				<RefreshCw />
				Refresh
			</Button>
		</div>
	</header>

	<div class="min-h-0 flex-1 overflow-y-auto bg-background">
		{#if loading && events.length === 0}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if events.length === 0}
			<div class="p-6">
				<EmptyState
					icon={ScrollText}
					title="No log events"
					description="Invoke the function or wait a few seconds, then refresh."
				/>
			</div>
		{:else}
			<ul class="divide-y divide-border/40">
				{#each events as e, idx (idx)}
					<li class="flex gap-3 px-4 py-1.5 font-mono text-xs">
						<span class="shrink-0 text-muted-foreground">{formatTs(e.timestamp)}</span>
						<span class="min-w-0 break-words whitespace-pre-wrap">{e.message}</span>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>
