<script lang="ts">
	/**
	 * Request Log — full-viewport view of the live SSE request stream.
	 * Reuses `dashboardState` (the same singleton powering the dashboard)
	 * so the buffer is shared across pages.
	 */
	import { onDestroy, onMount } from 'svelte';
	import { ServicePage } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { dashboardState } from '$lib/dashboard-state.svelte';
	import LogToolbar from '$lib/components/logs/log-toolbar.svelte';
	import RequestLogStream from '$lib/components/logs/request-log-stream.svelte';
	import {
		DEFAULT_COLUMNS,
		type ColumnKey,
		type LogTab,
	} from '$lib/components/logs/types';

	let tab = $state<LogTab>('all');
	let query = $state('');
	let visibleColumns = $state<Record<ColumnKey, boolean>>({ ...DEFAULT_COLUMNS });

	const total = $derived(dashboardState.events.length);
	const errorCount = $derived(
		dashboardState.events.filter((e) => e.status_code >= 400).length,
	);
	const slowCount = $derived(
		dashboardState.events.filter((e) => e.duration_ms > 200).length,
	);

	onMount(() => dashboardState.connect());
	onDestroy(() => dashboardState.disconnect());
</script>

<svelte:head>
	<title>AWSim · Request Log</title>
</svelte:head>

<ServicePage
	title="Request Log"
	description="Live tail of every request hitting the local emulator."
>
	{#snippet actions()}
		<Badge variant="outline" class="gap-1.5">
			<span
				class="size-1.5 rounded-full"
				class:bg-emerald-400={dashboardState.connectionStatus === 'open'}
				class:bg-amber-400={dashboardState.connectionStatus === 'paused' ||
					dashboardState.connectionStatus === 'connecting'}
				class:bg-muted-foreground={dashboardState.connectionStatus === 'closed'}
			></span>
			<span class="text-[11px]">{dashboardState.connectionStatus}</span>
		</Badge>
		<Badge variant="outline" class="text-[11px]">
			{total} total · {errorCount} err · {slowCount} slow
		</Badge>
	{/snippet}

	{#snippet toolbar()}
		<LogToolbar
			{tab}
			{query}
			{visibleColumns}
			onTabChange={(t) => (tab = t)}
			onQueryChange={(q) => (query = q)}
			onColumnToggle={(k, v) => (visibleColumns = { ...visibleColumns, [k]: v })}
		/>
	{/snippet}

	<RequestLogStream {tab} {query} {visibleColumns} />
</ServicePage>
