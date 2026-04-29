<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';
	import { batchGetTraces, type Trace, type TraceSummary } from '$lib/api/xray';
	import { toast } from 'svelte-sonner';

	interface Props {
		open: boolean;
		summary: TraceSummary | null;
		onOpenChange: (open: boolean) => void;
	}

	let { open, summary, onOpenChange }: Props = $props();

	let trace = $state<Trace | null>(null);
	let loading = $state(false);

	$effect(() => {
		if (open && summary) {
			void load(summary.id);
		} else if (!open) {
			trace = null;
		}
	});

	async function load(id: string) {
		loading = true;
		try {
			const traces = await batchGetTraces([id]);
			trace = traces[0] ?? null;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load trace');
		} finally {
			loading = false;
		}
	}

	function parseSegment(doc: string): Record<string, unknown> | null {
		try {
			return JSON.parse(doc);
		} catch {
			return null;
		}
	}

	function fmtDuration(d: number): string {
		if (d < 1) return `${(d * 1000).toFixed(0)} ms`;
		return `${d.toFixed(2)} s`;
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle>Trace details</SheetTitle>
			<SheetDescription>
				{#if summary}
					<span class="font-mono text-xs">{summary.id}</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 pb-4">
			{#if loading}
				<p class="text-sm text-muted-foreground">Loading…</p>
			{:else if trace && summary}
				<div class="flex flex-wrap items-center gap-2">
					{#if summary.hasFault}
						<Badge variant="outline" class="h-5 px-2 text-[10px] text-destructive">FAULT</Badge>
					{:else if summary.hasError}
						<Badge variant="outline" class="h-5 px-2 text-[10px] text-destructive">ERROR</Badge>
					{:else if summary.hasThrottle}
						<Badge variant="outline" class="h-5 px-2 text-[10px] text-amber-500">THROTTLE</Badge>
					{:else}
						<Badge variant="outline" class="h-5 px-2 text-[10px] text-green-500">OK</Badge>
					{/if}
					<Badge variant="outline" class="h-5 px-2 text-[10px]">
						{fmtDuration(trace.duration)}
					</Badge>
					<Badge variant="outline" class="h-5 px-2 text-[10px]">
						{trace.segments.length} segment{trace.segments.length === 1 ? '' : 's'}
					</Badge>
				</div>

				<div class="space-y-2">
					<div class="text-xs font-semibold text-muted-foreground">Segments</div>
					{#each trace.segments as seg (seg.id)}
						{@const doc = parseSegment(seg.document)}
						<div class="rounded-md border border-border p-3 text-xs">
							<div class="flex items-center justify-between">
								<span class="font-mono">{doc?.name ?? seg.id}</span>
								{#if doc?.start_time && doc?.end_time}
									<Badge variant="outline" class="h-4 px-2 text-[10px] font-mono">
										{(((doc.end_time as number) - (doc.start_time as number)) * 1000).toFixed(0)} ms
									</Badge>
								{/if}
							</div>
							{#if doc}
								<pre class="mt-2 max-h-[200px] overflow-auto rounded border border-border bg-muted/40 p-2 font-mono text-[10px] whitespace-pre-wrap">{JSON.stringify(doc, null, 2)}</pre>
							{:else}
								<pre class="mt-2 font-mono text-[10px] whitespace-pre-wrap">{seg.document}</pre>
							{/if}
						</div>
					{/each}
				</div>
			{/if}
		</div>
	</SheetContent>
</Sheet>
