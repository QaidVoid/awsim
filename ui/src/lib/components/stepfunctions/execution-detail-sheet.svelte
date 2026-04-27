<script lang="ts">
	import {
		describeExecution,
		getExecutionHistory,
		stopExecution,
		type Execution,
		type ExecutionDetail,
		type HistoryEvent
	} from '$lib/api/stepfunctions';
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import { toast } from 'svelte-sonner';
	import StopCircle from '@lucide/svelte/icons/stop-circle';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		execution: Execution | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { execution, open, onOpenChange }: Props = $props();

	let detail = $state<ExecutionDetail | null>(null);
	let events = $state<HistoryEvent[]>([]);
	let loading = $state(false);
	let lastArn = $state('');
	let detailTab = $state<'timeline' | 'io'>('timeline');

	$effect(() => {
		if (open && execution && execution.arn !== lastArn) {
			lastArn = execution.arn;
			void load();
		}
		if (!open) {
			lastArn = '';
		}
	});

	async function load() {
		if (!execution) return;
		loading = true;
		detail = null;
		events = [];
		try {
			const [d, h] = await Promise.all([
				describeExecution(execution.arn),
				getExecutionHistory(execution.arn)
			]);
			detail = d;
			events = h.events;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load execution');
		} finally {
			loading = false;
		}
	}

	async function handleStop() {
		if (!execution) return;
		if (!confirm(`Stop execution ${execution.name}?`)) return;
		try {
			await stopExecution(execution.arn);
			toast.success('Stop requested');
			await load();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Stop failed');
		}
	}

	function statusVariant(s: string): 'default' | 'secondary' | 'destructive' | 'outline' {
		if (s === 'SUCCEEDED') return 'default';
		if (s === 'FAILED' || s === 'TIMED_OUT' || s === 'ABORTED') return 'destructive';
		if (s === 'RUNNING') return 'secondary';
		return 'outline';
	}

	function pretty(s?: string): string {
		if (!s) return '—';
		try {
			return JSON.stringify(JSON.parse(s), null, 2);
		} catch {
			return s;
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

	function eventDot(type: string): string {
		if (type.includes('Succeeded')) return 'bg-emerald-500';
		if (type.includes('Failed') || type.includes('Aborted') || type.includes('TimedOut'))
			return 'bg-destructive';
		if (type.includes('Started') || type.includes('Entered')) return 'bg-blue-500';
		if (type.includes('Exited')) return 'bg-orange-500';
		return 'bg-muted-foreground';
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full overflow-y-auto sm:max-w-2xl">
		{#if execution}
			<SheetHeader class="gap-1">
				<div class="flex items-center justify-between gap-2">
					<SheetTitle class="truncate font-mono text-base">
						{execution.name || execution.arn.split(':').pop()}
					</SheetTitle>
					{#if detail}
						<Badge variant={statusVariant(detail.status)}>{detail.status}</Badge>
					{/if}
				</div>
				<SheetDescription class="truncate font-mono text-[11px]">
					{execution.arn}
				</SheetDescription>
			</SheetHeader>

			<div class="flex items-center justify-end gap-2 px-4 pt-2">
				<Button type="button" variant="outline" size="sm" onclick={load} disabled={loading}>
					<RefreshCw />
					Refresh
				</Button>
				{#if detail?.status === 'RUNNING'}
					<Button type="button" variant="destructive" size="sm" onclick={handleStop}>
						<StopCircle />
						Stop
					</Button>
				{/if}
			</div>

			{#if loading}
				<div class="flex h-32 items-center justify-center text-muted-foreground">
					<Loader2 class="size-4 animate-spin" />
				</div>
			{:else if detail}
				<Tabs bind:value={detailTab} class="flex min-h-0 flex-1 flex-col">
					<TabsList class="mx-4 mt-2 self-start">
						<TabsTrigger value="timeline">Timeline ({events.length})</TabsTrigger>
						<TabsTrigger value="io">Input / Output</TabsTrigger>
					</TabsList>

					<TabsContent value="timeline" class="m-0 px-4 py-3">
						{#if events.length === 0}
							<p class="py-12 text-center text-xs text-muted-foreground">
								No history events yet.
							</p>
						{:else}
							<ol class="flex flex-col gap-0.5">
								{#each events as ev (ev.id)}
									<li class="flex items-start gap-2 rounded px-2 py-1.5 hover:bg-muted/40">
										<span
											class="mt-1.5 size-2 shrink-0 rounded-full {eventDot(ev.type)}"
										></span>
										<div class="min-w-0 flex-1">
											<div class="flex items-center justify-between gap-2">
												<div class="flex items-center gap-2">
													<span class="font-mono text-xs">{ev.type}</span>
													{#if ev.stateName}
														<Badge variant="outline" class="text-[10px]">
															{ev.stateName}
														</Badge>
													{/if}
												</div>
												<span class="text-[10px] text-muted-foreground">
													#{ev.id} · {formatTs(ev.timestamp)}
												</span>
											</div>
											{#if ev.error}
												<div class="mt-1 text-[11px] text-destructive">
													{ev.error}{ev.cause ? ` — ${ev.cause}` : ''}
												</div>
											{:else if ev.output}
												<pre
													class="mt-1 max-h-24 overflow-auto rounded bg-muted/40 p-1.5 font-mono text-[10px]">{pretty(
														ev.output
													)}</pre>
											{:else if ev.input}
												<pre
													class="mt-1 max-h-24 overflow-auto rounded bg-muted/40 p-1.5 font-mono text-[10px]">{pretty(
														ev.input
													)}</pre>
											{/if}
										</div>
									</li>
								{/each}
							</ol>
						{/if}
					</TabsContent>

					<TabsContent value="io" class="m-0 px-4 py-3">
						<div class="grid grid-cols-1 gap-3">
							<section>
								<div class="mb-1 text-[10px] tracking-wide text-muted-foreground uppercase">
									Input
								</div>
								<pre
									class="max-h-64 overflow-auto rounded-md border border-border bg-muted/40 p-3 font-mono text-xs">{pretty(
										detail.input
									)}</pre>
							</section>
							<section>
								<div class="mb-1 text-[10px] tracking-wide text-muted-foreground uppercase">
									Output
								</div>
								<pre
									class="max-h-64 overflow-auto rounded-md border border-border bg-muted/40 p-3 font-mono text-xs">{pretty(
										detail.output
									)}</pre>
							</section>
							{#if detail.error}
								<section>
									<div
										class="mb-1 text-[10px] tracking-wide text-destructive uppercase"
									>
										Error
									</div>
									<pre
										class="overflow-auto rounded-md border border-destructive/40 bg-destructive/10 p-3 font-mono text-xs text-destructive">{detail.error}{detail.cause
											? `\n${detail.cause}`
											: ''}</pre>
								</section>
							{/if}
						</div>
					</TabsContent>
				</Tabs>
			{/if}
		{/if}
	</SheetContent>
</Sheet>
