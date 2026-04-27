<script lang="ts">
	import type { StateMachineDetail } from '$lib/api/stepfunctions';
	import AslViewer from '$lib/components/AslViewer.svelte';
	import { Button } from '$lib/components/ui/button';
	import Code from '@lucide/svelte/icons/code';
	import Workflow from '@lucide/svelte/icons/workflow';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		machine: StateMachineDetail | null;
		loading: boolean;
	}

	let { machine, loading }: Props = $props();

	let view = $state<'diagram' | 'json'>('diagram');

	function pretty(json: string): string {
		try {
			return JSON.stringify(JSON.parse(json), null, 2);
		} catch {
			return json;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header
		class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2"
	>
		<div class="text-xs text-muted-foreground">Amazon States Language definition</div>
		<div class="flex items-center gap-1 rounded-md border border-border p-0.5">
			<Button
				type="button"
				variant={view === 'diagram' ? 'secondary' : 'ghost'}
				size="xs"
				onclick={() => (view = 'diagram')}
			>
				<Workflow />
				Diagram
			</Button>
			<Button
				type="button"
				variant={view === 'json' ? 'secondary' : 'ghost'}
				size="xs"
				onclick={() => (view = 'json')}
			>
				<Code />
				JSON
			</Button>
		</div>
	</header>

	<div class="min-h-0 flex-1 overflow-y-auto p-4">
		{#if loading}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if !machine}
			<div class="flex h-32 items-center justify-center text-xs text-muted-foreground">
				No state machine selected.
			</div>
		{:else if view === 'diagram'}
			<AslViewer definition={machine.definition} />
		{:else}
			<pre
				class="max-h-[70vh] overflow-auto rounded-md border border-border bg-muted/40 p-3 font-mono text-xs">{pretty(
					machine.definition
				)}</pre>
		{/if}
	</div>
</div>
