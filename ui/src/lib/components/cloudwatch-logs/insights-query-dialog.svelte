<script lang="ts">
	/**
	 * CloudWatch Logs Insights query dialog.
	 *
	 * Lets the user run a query string against the currently selected log
	 * group, polls GetQueryResults until the query completes, and renders
	 * the resulting tabular rows.
	 */
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle,
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import { toast } from 'svelte-sonner';
	import {
		startQuery,
		getQueryResults,
		type InsightsQueryStatus,
	} from '$lib/api/cloudwatch-logs';

	interface Props {
		open: boolean;
		group: string | null;
		onOpenChange: (open: boolean) => void;
	}

	let { open, group, onOpenChange }: Props = $props();

	const DEFAULT_QUERY = `fields @timestamp, @message
| sort @timestamp desc
| limit 50`;

	let queryString = $state(DEFAULT_QUERY);
	let rangeMins = $state(60);
	let running = $state(false);
	let result = $state<InsightsQueryStatus | null>(null);
	let error = $state<string | null>(null);

	const columns = $derived.by(() => {
		if (!result || result.results.length === 0) return [] as string[];
		const seen = new Set<string>();
		const order: string[] = [];
		for (const row of result.results) {
			for (const cell of row) {
				if (!seen.has(cell.field)) {
					seen.add(cell.field);
					order.push(cell.field);
				}
			}
		}
		return order;
	});

	async function run() {
		if (!group) return;
		running = true;
		error = null;
		result = null;
		try {
			const now = Math.floor(Date.now() / 1000);
			const start = now - Math.max(1, rangeMins) * 60;
			const { queryId } = await startQuery(group, queryString, start, now);
			if (!queryId) {
				throw new Error('No queryId returned');
			}
			// Poll up to ~30s for completion.
			for (let i = 0; i < 30; i++) {
				const status = await getQueryResults(queryId);
				result = status;
				if (
					status.status === 'Complete' ||
					status.status === 'Failed' ||
					status.status === 'Cancelled' ||
					status.status === 'Timeout'
				) {
					break;
				}
				await new Promise((r) => setTimeout(r, 1000));
			}
		} catch (e) {
			error = e instanceof Error ? e.message : 'Query failed';
			toast.error(error);
		} finally {
			running = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-3xl">
		<DialogHeader>
			<DialogTitle>Logs Insights</DialogTitle>
			<DialogDescription>
				Run a Logs Insights query against
				<span class="font-mono text-foreground">{group ?? '—'}</span>.
			</DialogDescription>
		</DialogHeader>

		<div class="space-y-3">
			<div class="space-y-1.5">
				<Label for="insights-query" class="text-xs">Query</Label>
				<Textarea
					id="insights-query"
					bind:value={queryString}
					rows={6}
					class="font-mono text-xs"
				/>
			</div>
			<div class="flex items-end gap-2">
				<div class="space-y-1.5">
					<Label for="insights-range" class="text-xs">Range (minutes)</Label>
					<input
						id="insights-range"
						type="number"
						min="1"
						max="10080"
						bind:value={rangeMins}
						class="h-8 w-24 rounded-md border border-input bg-transparent px-2 text-xs"
					/>
				</div>
				<Button onclick={run} disabled={running || !group} class="h-8 text-xs">
					{running ? 'Running…' : 'Run query'}
				</Button>
				{#if result}
					<span class="ml-auto text-[11px] text-muted-foreground">
						{result.status}
						{#if result.statistics?.recordsScanned != null}
							· {result.statistics.recordsScanned} scanned
						{/if}
					</span>
				{/if}
			</div>

			{#if error}
				<div class="rounded-md border border-rose-500/30 bg-rose-500/10 p-2 text-xs text-rose-400">
					{error}
				</div>
			{/if}

			{#if result && result.results.length > 0}
				<div class="max-h-72 overflow-auto rounded-md border border-border">
					<table class="w-full text-xs">
						<thead class="sticky top-0 bg-muted/40">
							<tr>
								{#each columns as c (c)}
									<th class="px-2 py-1.5 text-left font-medium text-muted-foreground">{c}</th>
								{/each}
							</tr>
						</thead>
						<tbody>
							{#each result.results as row, i (i)}
								<tr class="border-t border-border/40">
									{#each columns as c (c)}
										<td class="px-2 py-1 font-mono">
											{row.find((cell) => cell.field === c)?.value ?? ''}
										</td>
									{/each}
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{:else if result}
				<div class="rounded-md border border-dashed border-border p-3 text-center text-xs text-muted-foreground">
					Query returned no rows.
				</div>
			{/if}
		</div>

		<DialogFooter>
			<Button variant="ghost" onclick={() => onOpenChange(false)}>Close</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
