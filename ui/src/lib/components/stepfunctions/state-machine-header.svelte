<script lang="ts">
	import type { StateMachineDetail } from '$lib/api/stepfunctions';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';

	interface Props {
		machine: StateMachineDetail;
		onDelete: () => void;
		onRefresh: () => void;
	}

	let { machine, onDelete, onRefresh }: Props = $props();

	function statusVariant(s: string): 'default' | 'secondary' | 'destructive' | 'outline' {
		if (!s) return 'outline';
		if (s === 'ACTIVE') return 'default';
		if (s === 'DELETING') return 'destructive';
		return 'secondary';
	}
</script>

<div
	class="flex items-start justify-between gap-3 border-b border-border bg-background/40 px-4 py-3"
>
	<div class="min-w-0">
		<div class="flex items-center gap-2">
			<h2 class="truncate font-mono text-base font-semibold">{machine.name}</h2>
			{#if machine.status}
				<Badge variant={statusVariant(machine.status)}>{machine.status}</Badge>
			{/if}
			<Badge variant="outline">{machine.type}</Badge>
		</div>
		<div class="mt-1 truncate font-mono text-[11px] text-muted-foreground">{machine.arn}</div>
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
