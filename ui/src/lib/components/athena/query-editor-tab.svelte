<script lang="ts">
	import {
		startQueryExecution,
		getQueryExecution,
		getQueryResults,
		type QueryExecution,
		type QueryResults,
	} from '$lib/api/athena';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import PlayIcon from '@lucide/svelte/icons/play';
	import StopCircleIcon from '@lucide/svelte/icons/stop-circle';
	import DatabaseIcon from '@lucide/svelte/icons/database';
	import { toast } from 'svelte-sonner';

	let queryString = $state('SELECT 1');
	let workGroup = $state('primary');
	let database = $state('');
	let outputLocation = $state('');
	let busy = $state(false);
	let exec = $state<QueryExecution | null>(null);
	let results = $state<QueryResults | null>(null);
	let pollHandle: ReturnType<typeof setInterval> | null = null;

	function statusVariant(s: string): 'secondary' | 'destructive' | 'outline' {
		if (s === 'SUCCEEDED') return 'secondary';
		if (s === 'FAILED' || s === 'CANCELLED') return 'destructive';
		return 'outline';
	}

	function clearPoll() {
		if (pollHandle !== null) {
			clearInterval(pollHandle);
			pollHandle = null;
		}
	}

	async function pollOnce(id: string) {
		try {
			exec = await getQueryExecution(id);
			const state = exec.status.state;
			if (state === 'SUCCEEDED') {
				clearPoll();
				try {
					results = await getQueryResults(id, 1000);
				} catch (e) {
					toast.error(e instanceof Error ? e.message : 'Failed to fetch results');
				} finally {
					busy = false;
				}
			} else if (state === 'FAILED' || state === 'CANCELLED') {
				clearPoll();
				busy = false;
				toast.error(exec.status.stateChangeReason ?? `Query ${state.toLowerCase()}`);
			}
		} catch (e) {
			clearPoll();
			busy = false;
			toast.error(e instanceof Error ? e.message : 'Failed to poll execution');
		}
	}

	async function run() {
		if (!queryString.trim() || busy) return;
		clearPoll();
		results = null;
		exec = null;
		busy = true;
		try {
			const id = await startQueryExecution({
				queryString: queryString.trim(),
				workGroup: workGroup.trim() || undefined,
				database: database.trim() || undefined,
				outputLocation: outputLocation.trim() || undefined,
			});
			toast.success('Query submitted.');
			await pollOnce(id);
			if (busy) {
				pollHandle = setInterval(() => pollOnce(id), 1500);
			}
		} catch (e) {
			busy = false;
			toast.error(e instanceof Error ? e.message : 'Failed to start query');
		}
	}

	async function stop() {
		if (!exec) return;
		clearPoll();
		try {
			const { stopQueryExecution } = await import('$lib/api/athena');
			await stopQueryExecution(exec.queryExecutionId);
			toast.success('Stop requested.');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to stop');
		} finally {
			busy = false;
		}
	}
</script>

<div class="flex flex-col gap-4 p-4">
	<div class="flex flex-wrap items-end gap-3">
		<div class="flex min-w-32 flex-col gap-1">
			<Label for="athena-wg">WorkGroup</Label>
			<Input id="athena-wg" bind:value={workGroup} class="h-8 text-xs" />
		</div>
		<div class="flex min-w-40 flex-col gap-1">
			<Label for="athena-db">Database (optional)</Label>
			<Input id="athena-db" bind:value={database} class="h-8 text-xs" />
		</div>
		<div class="flex min-w-64 flex-1 flex-col gap-1">
			<Label for="athena-output">Output S3 location (optional)</Label>
			<Input
				id="athena-output"
				bind:value={outputLocation}
				placeholder="s3://my-bucket/athena/"
				class="h-8 text-xs"
			/>
		</div>
		<Button onclick={run} disabled={busy || !queryString.trim()}>
			<PlayIcon />
			{busy ? 'Running…' : 'Run query'}
		</Button>
		{#if busy}
			<Button variant="outline" onclick={stop}>
				<StopCircleIcon />
				Stop
			</Button>
		{/if}
	</div>

	<div class="flex flex-col gap-1">
		<Label for="athena-sql">SQL</Label>
		<Textarea
			id="athena-sql"
			bind:value={queryString}
			rows={8}
			class="font-mono text-xs"
			placeholder="SELECT * FROM …"
		/>
	</div>

	{#if exec}
		<div class="flex flex-wrap items-center gap-2 text-xs">
			<Badge variant={statusVariant(exec.status.state)} class="h-4 px-1 text-[10px]">
				{exec.status.state || '—'}
			</Badge>
			<span class="font-mono text-muted-foreground">{exec.queryExecutionId}</span>
			{#if exec.statistics}
				<span class="text-muted-foreground">
					{exec.statistics.engineExecutionTimeInMillis} ms ·
					{exec.statistics.dataScannedInBytes} bytes scanned
				</span>
			{/if}
		</div>
		{#if exec.status.stateChangeReason}
			<p class="text-xs text-destructive">{exec.status.stateChangeReason}</p>
		{/if}
	{/if}

	{#if results}
		<div class="overflow-auto rounded-md border border-border">
			<table class="w-full border-collapse text-xs">
				<thead class="bg-muted/40">
					<tr>
						{#each results.columns as col (col.name)}
							<th class="border-b border-border px-3 py-2 text-left font-medium">
								<div>{col.name}</div>
								<div class="text-[10px] font-normal text-muted-foreground">{col.type}</div>
							</th>
						{/each}
					</tr>
				</thead>
				<tbody>
					{#each results.rows as row, i (i)}
						<tr class="border-b border-border/40">
							{#each row as cell, j (j)}
								<td class="px-3 py-1.5 font-mono">{cell}</td>
							{/each}
						</tr>
					{:else}
						<tr>
							<td
								colspan={results.columns.length || 1}
								class="px-3 py-6 text-center text-muted-foreground"
							>
								Query returned no rows.
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{:else if !exec && !busy}
		<EmptyState
			icon={DatabaseIcon}
			title="Run a query"
			description="Submit a SQL statement to Athena and view the result rows here."
		/>
	{/if}
</div>
