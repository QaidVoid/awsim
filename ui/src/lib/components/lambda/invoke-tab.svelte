<script lang="ts">
	import { invokeFunction, type InvokeResult } from '$lib/api/lambda';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import { toast } from 'svelte-sonner';
	import Play from '@lucide/svelte/icons/play';
	import Eraser from '@lucide/svelte/icons/eraser';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import ScrollText from '@lucide/svelte/icons/scroll-text';

	interface Props {
		functionName: string;
		/** Jump the page to this function's Logs tab. */
		onViewLogs?: () => void;
	}

	let { functionName, onViewLogs }: Props = $props();

	interface InvokeRun {
		id: number;
		ts: number;
		payload: string;
		result: InvokeResult;
	}

	let payload = $state('{}');
	let invoking = $state(false);
	let result = $state<InvokeResult | null>(null);
	let history = $state<InvokeRun[]>([]);
	let activeRunId = $state<number | null>(null);
	let lastFn = $state('');
	let nextId = 0;

	// History is per-function: reset everything when the selected
	// function changes.
	$effect(() => {
		if (functionName !== lastFn) {
			lastFn = functionName;
			result = null;
			history = [];
			activeRunId = null;
		}
	});

	function format(text: string): string {
		try {
			return JSON.stringify(JSON.parse(text), null, 2);
		} catch {
			return text;
		}
	}

	function relTime(ts: number): string {
		const s = Math.max(0, Math.round((Date.now() - ts) / 1000));
		if (s < 60) return `${s}s ago`;
		if (s < 3600) return `${Math.floor(s / 60)}m ago`;
		return `${Math.floor(s / 3600)}h ago`;
	}

	async function handleInvoke() {
		invoking = true;
		try {
			const r = await invokeFunction(functionName, payload || '{}');
			result = r;
			const run: InvokeRun = {
				id: nextId++,
				ts: Date.now(),
				payload: payload || '{}',
				result: r
			};
			// Newest first, cap the ring so a long tweak-run loop
			// doesn't grow unbounded.
			history = [run, ...history].slice(0, 20);
			activeRunId = run.id;
			if (r.functionError) {
				toast.error(`Function error: ${r.functionError}`);
			} else {
				toast.success(`Invoked in ${r.durationMs} ms`);
			}
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Invoke failed');
		} finally {
			invoking = false;
		}
	}

	// Load a past run back into the editor + result so the user can
	// inspect it or tweak the payload and re-invoke.
	function openRun(run: InvokeRun) {
		payload = run.payload;
		result = run.result;
		activeRunId = run.id;
	}

	function clearAll() {
		payload = '{}';
		result = null;
		activeRunId = null;
	}

	function clearHistory() {
		history = [];
	}
</script>

<div class="flex flex-col gap-3 p-4">
	{#if history.length}
		<section class="rounded-md border border-border bg-card">
			<header
				class="flex items-center justify-between border-b border-border px-3 py-2"
			>
				<h3 class="text-xs font-medium uppercase tracking-wide text-muted-foreground">
					Recent invocations
				</h3>
				<button
					type="button"
					class="text-[11px] text-muted-foreground hover:text-foreground"
					onclick={clearHistory}
				>
					Clear history
				</button>
			</header>
			<div class="flex gap-1.5 overflow-x-auto p-2">
				{#each history as run (run.id)}
					{@const err = !!run.result.functionError}
					<button
						type="button"
						onclick={() => openRun(run)}
						class="flex shrink-0 items-center gap-1.5 rounded-md border px-2 py-1 text-xs transition-colors {run.id ===
						activeRunId
							? 'border-ring/50 bg-muted'
							: 'border-border hover:bg-muted/50'}"
						title={`${relTime(run.ts)} - ${err ? run.result.functionError : 'ok'}`}
					>
						<span
							class="size-1.5 rounded-full {err ? 'bg-destructive' : 'bg-emerald-500'}"
						></span>
						<span class="font-mono">{run.result.durationMs}ms</span>
						<span class="text-muted-foreground">{relTime(run.ts)}</span>
					</button>
				{/each}
			</div>
		</section>
	{/if}

	<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
		<section class="flex flex-col rounded-md border border-border bg-card">
			<header class="border-b border-border px-4 py-3">
				<h3 class="text-sm font-medium">Event payload</h3>
				<p class="mt-0.5 text-xs text-muted-foreground">
					JSON sent to the function as the event.
				</p>
			</header>
			<div class="flex flex-1 flex-col gap-2 px-4 py-3">
				<Label for="invoke-payload" class="sr-only">Payload</Label>
				<textarea
					id="invoke-payload"
					bind:value={payload}
					rows="14"
					spellcheck="false"
					class="min-h-[280px] flex-1 resize-y rounded-md border border-border bg-background p-3 font-mono text-xs outline-none focus:border-ring focus:ring-1 focus:ring-ring"
				></textarea>
			</div>
			<footer class="flex items-center justify-end gap-2 border-t border-border px-4 py-3">
				<Button type="button" variant="ghost" size="sm" onclick={clearAll}>
					<Eraser />
					Clear
				</Button>
				<Button type="button" onclick={handleInvoke} disabled={invoking}>
					<Play />
					{invoking ? 'Invoking...' : 'Invoke'}
				</Button>
			</footer>
		</section>

		<section class="flex flex-col rounded-md border border-border bg-card">
			<header class="border-b border-border px-4 py-3">
				<div class="flex items-center justify-between gap-2">
					<div>
						<h3 class="text-sm font-medium">Result</h3>
						<p class="mt-0.5 text-xs text-muted-foreground">
							Response payload + tail of CloudWatch logs.
						</p>
					</div>
					{#if result}
						<div class="flex items-center gap-1.5 text-xs">
							{#if result.functionError}
								<AlertTriangle class="size-3.5 text-destructive" />
								<span class="text-destructive">{result.functionError}</span>
							{:else}
								<CheckCircle2 class="size-3.5 text-emerald-500" />
								<span class="text-muted-foreground">{result.durationMs} ms</span>
							{/if}
						</div>
					{/if}
				</div>
			</header>
			<div class="flex flex-1 flex-col gap-3 px-4 py-3">
				{#if !result}
					<p class="py-12 text-center text-xs text-muted-foreground">
						Run the function to see its response.
					</p>
				{:else}
					<div>
						<div
							class="mb-1 flex items-center justify-between text-[10px] tracking-wide text-muted-foreground uppercase"
						>
							<span>Payload</span>
							<span>HTTP {result.statusCode}</span>
						</div>
						<pre
							class="max-h-64 overflow-auto rounded-md bg-muted/40 p-3 font-mono text-xs">{format(
								result.payload
							) || '—'}</pre>
					</div>
					{#if result.logTail}
						<div>
							<div
								class="mb-1 flex items-center justify-between text-[10px] tracking-wide text-muted-foreground uppercase"
							>
								<span>Log tail</span>
								{#if onViewLogs}
									<button
										type="button"
										class="flex items-center gap-1 text-[11px] normal-case text-muted-foreground hover:text-foreground"
										onclick={onViewLogs}
									>
										<ScrollText class="size-3" />
										View full logs
									</button>
								{/if}
							</div>
							<pre
								class="max-h-64 overflow-auto rounded-md bg-muted/40 p-3 font-mono text-xs">{result.logTail}</pre>
						</div>
					{:else if onViewLogs}
						<button
							type="button"
							class="flex items-center justify-center gap-1 rounded-md border border-dashed border-border py-2 text-xs text-muted-foreground hover:text-foreground"
							onclick={onViewLogs}
						>
							<ScrollText class="size-3.5" />
							View full logs
						</button>
					{/if}
				{/if}
			</div>
		</section>
	</div>
</div>
