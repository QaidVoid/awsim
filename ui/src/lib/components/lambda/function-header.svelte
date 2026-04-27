<script lang="ts">
	import type { LambdaConfiguration } from '$lib/api/lambda';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';

	interface Props {
		config: LambdaConfiguration;
		onDelete: () => void;
		onRefresh: () => void;
	}

	let { config, onDelete, onRefresh }: Props = $props();

	function stateVariant(s: string): 'default' | 'secondary' | 'destructive' | 'outline' {
		if (!s) return 'outline';
		if (s === 'Active') return 'default';
		if (s === 'Failed') return 'destructive';
		return 'secondary';
	}
</script>

<div
	class="flex items-start justify-between gap-3 border-b border-border bg-background/40 px-4 py-3"
>
	<div class="min-w-0">
		<div class="flex items-center gap-2">
			<h2 class="truncate font-mono text-base font-semibold">{config.name}</h2>
			{#if config.state}
				<Badge variant={stateVariant(config.state)}>{config.state}</Badge>
			{/if}
			<Badge variant="outline">{config.version}</Badge>
		</div>
		<div
			class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground"
		>
			<span><span class="text-foreground/70">Runtime:</span> {config.runtime || '—'}</span>
			<span
				><span class="text-foreground/70">Handler:</span>
				<span class="font-mono">{config.handler || '—'}</span></span
			>
			<span><span class="text-foreground/70">Memory:</span> {config.memorySize} MB</span>
			<span><span class="text-foreground/70">Timeout:</span> {config.timeout}s</span>
		</div>
	</div>
	<div class="flex shrink-0 items-center gap-2">
		<Button
			type="button"
			variant="outline"
			size="icon-sm"
			onclick={onRefresh}
			aria-label="Refresh"
		>
			<RefreshCw />
		</Button>
		<Button type="button" variant="destructive" size="sm" onclick={onDelete}>
			<Trash2 />
			Delete
		</Button>
	</div>
</div>
