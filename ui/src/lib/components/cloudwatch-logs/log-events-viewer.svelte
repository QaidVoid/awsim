<script lang="ts">
	/**
	 * Right pane: log events for a selected stream with optional auto-tail
	 * (polls every 2s) and a free-form regex filter.
	 */
	import { onDestroy } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Switch } from '$lib/components/ui/switch';
	import { Label } from '$lib/components/ui/label';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import FileText from '@lucide/svelte/icons/file-text';
	import { getLogEvents, type LogEvent } from '$lib/api/cloudwatch-logs';
	import { EmptyState } from '$lib/components/service';
	import { cn } from '$lib/utils';

	interface Props {
		group: string | null;
		stream: string | null;
	}

	let { group, stream }: Props = $props();

	let events = $state<LogEvent[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let filter = $state('');
	let autoTail = $state(false);
	let pollTimer: ReturnType<typeof setInterval> | null = null;

	const filterRegex = $derived.by(() => {
		if (!filter.trim()) return null;
		try {
			return new RegExp(filter, 'i');
		} catch {
			return null;
		}
	});

	const visible = $derived.by(() => {
		if (!filterRegex) return events;
		return events.filter((e) => filterRegex.test(e.message));
	});

	async function load() {
		if (!group || !stream) {
			events = [];
			return;
		}
		loading = true;
		error = null;
		try {
			const data = await getLogEvents(group, stream, 200);
			events = data.events.sort((a, b) => b.timestamp - a.timestamp);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load events';
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		// Reload when group/stream changes.
		void group;
		void stream;
		load();
	});

	$effect(() => {
		if (pollTimer) {
			clearInterval(pollTimer);
			pollTimer = null;
		}
		if (autoTail && group && stream) {
			pollTimer = setInterval(() => load(), 2000);
		}
	});

	onDestroy(() => {
		if (pollTimer) clearInterval(pollTimer);
	});

	function ts(ms: number): string {
		try {
			const d = new Date(ms);
			return (
				d.toLocaleTimeString(undefined, { hour12: false }) +
				'.' +
				String(d.getMilliseconds()).padStart(3, '0')
			);
		} catch {
			return String(ms);
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header
		class="flex shrink-0 flex-wrap items-center gap-2 border-b border-border px-3 py-2"
	>
		<h2 class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
			Events
			{#if visible.length}
				· {visible.length}
			{/if}
		</h2>
		<div class="ml-auto flex flex-wrap items-center gap-2">
			<Input
				bind:value={filter}
				placeholder="filter regex…"
				class="h-7 w-44 text-xs"
				aria-label="Filter events"
			/>
			<div class="flex items-center gap-1.5">
				<Switch id="autotail" bind:checked={autoTail} />
				<Label for="autotail" class="text-xs">Auto-tail (2s)</Label>
			</div>
			<Button
				size="sm"
				variant="ghost"
				class="h-7 px-2"
				onclick={load}
				disabled={!group || !stream}
				title="Refresh"
				aria-label="Refresh events"
			>
				<RefreshCw class={cn('size-3.5', loading && 'animate-spin')} />
			</Button>
		</div>
	</header>

	<div class="min-h-0 flex-1 overflow-auto bg-muted/10">
		{#if !group || !stream}
			<div class="p-6">
				<EmptyState
					icon={FileText}
					title="No stream selected"
					description="Pick a stream to view its events."
				/>
			</div>
		{:else if error}
			<div class="m-4 rounded-md border border-rose-500/30 bg-rose-500/10 p-3 text-xs text-rose-400">
				{error}
			</div>
		{:else if visible.length === 0 && !loading}
			<div class="p-6">
				<EmptyState
					icon={FileText}
					title={filter ? 'No matches' : 'No events'}
					description={filter ? 'No events match this filter.' : 'This stream has no events.'}
				/>
			</div>
		{:else}
			<ol class="divide-y divide-border/40 font-mono text-[11px]">
				{#each visible as e (e.eventId ?? `${e.timestamp}-${e.message.slice(0, 16)}`)}
					<li class="flex items-start gap-3 px-3 py-1.5 hover:bg-muted/30">
						<span class="shrink-0 text-muted-foreground">{ts(e.timestamp)}</span>
						<span class="whitespace-pre-wrap break-all text-foreground/90">{e.message}</span>
					</li>
				{/each}
			</ol>
		{/if}
	</div>
</div>
