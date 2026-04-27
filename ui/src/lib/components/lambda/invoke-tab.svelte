<script lang="ts">
	import { invokeFunction, type InvokeResult } from '$lib/api/lambda';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import { toast } from 'svelte-sonner';
	import Play from '@lucide/svelte/icons/play';
	import Eraser from '@lucide/svelte/icons/eraser';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';

	interface Props {
		functionName: string;
	}

	let { functionName }: Props = $props();

	let payload = $state('{}');
	let invoking = $state(false);
	let result = $state<InvokeResult | null>(null);
	let lastFn = $state('');

	$effect(() => {
		if (functionName !== lastFn) {
			lastFn = functionName;
			result = null;
		}
	});

	function format(text: string): string {
		try {
			return JSON.stringify(JSON.parse(text), null, 2);
		} catch {
			return text;
		}
	}

	async function handleInvoke() {
		invoking = true;
		try {
			const r = await invokeFunction(functionName, payload || '{}');
			result = r;
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

	function clearAll() {
		payload = '{}';
		result = null;
	}
</script>

<div class="grid grid-cols-1 gap-4 p-4 lg:grid-cols-2">
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
							class="mb-1 text-[10px] tracking-wide text-muted-foreground uppercase"
						>
							Log tail
						</div>
						<pre
							class="max-h-64 overflow-auto rounded-md bg-muted/40 p-3 font-mono text-xs">{result.logTail}</pre>
					</div>
				{/if}
			{/if}
		</div>
	</section>
</div>
