<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { executeStatement, attributeToString, type Item } from '$lib/api/dynamodb';
	import { Button } from '$lib/components/ui/button';
	import { Textarea } from '$lib/components/ui/textarea';
	import { EmptyState } from '$lib/components/service';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Play from '@lucide/svelte/icons/play';
	import Database from '@lucide/svelte/icons/database';

	interface Props {
		tableName: string;
	}

	let { tableName }: Props = $props();

	let statement = $state('');
	let running = $state(false);
	let items = $state<Item[]>([]);
	let nextToken = $state<string | undefined>(undefined);
	let lastError = $state<string | null>(null);
	let consumed = $state<{ capacityUnits: number; readUnits: number; writeUnits: number } | null>(
		null
	);

	$effect(() => {
		if (tableName) {
			statement = `SELECT * FROM "${tableName}"`;
			items = [];
			nextToken = undefined;
			lastError = null;
			consumed = null;
		}
	});

	let columns = $derived.by(() => {
		const seen = new Set<string>();
		const ordered: string[] = [];
		for (const item of items) {
			for (const k of Object.keys(item)) {
				if (!seen.has(k)) {
					seen.add(k);
					ordered.push(k);
				}
			}
		}
		return ordered;
	});

	async function run() {
		running = true;
		lastError = null;
		try {
			const res = await executeStatement(statement);
			items = res.items;
			nextToken = res.nextToken;
			consumed = res.consumedCapacity ?? null;
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Statement failed';
			lastError = msg;
			toast.error(msg);
		} finally {
			running = false;
		}
	}

	function handleKey(e: KeyboardEvent) {
		if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
			e.preventDefault();
			void run();
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="shrink-0 border-b border-border bg-background/40 p-3">
		<Textarea
			bind:value={statement}
			rows={4}
			class="font-mono text-xs"
			placeholder="SELECT * FROM &quot;TableName&quot; WHERE ..."
			onkeydown={handleKey}
		/>
		<div class="mt-2 flex items-center justify-between">
			<span class="text-[11px] text-muted-foreground">
				PartiQL · Cmd/Ctrl+Enter to run
				{#if consumed}
					· consumed {consumed.capacityUnits} CU{consumed.readUnits
						? ` (${consumed.readUnits} read)`
						: ''}{consumed.writeUnits ? ` (${consumed.writeUnits} write)` : ''}
				{/if}
			</span>
			<Button size="sm" onclick={run} disabled={running}>
				{#if running}
					<Loader2 class="size-3.5 animate-spin" />
				{:else}
					<Play class="size-3.5" />
				{/if}
				Run
			</Button>
		</div>
		{#if lastError}
			<p class="mt-2 rounded-md bg-destructive/10 px-2 py-1 text-[11px] text-destructive">
				{lastError}
			</p>
		{/if}
	</div>

	<div class="min-h-0 flex-1 overflow-hidden">
		{#if items.length === 0 && !running}
			<div class="flex h-full items-center justify-center p-6">
				<EmptyState
					icon={Database}
					title="No results yet"
					description="Run a PartiQL statement to see rows here."
				/>
			</div>
		{:else}
			<div class="h-full overflow-auto">
				<table class="w-full min-w-max text-xs">
					<thead
						class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm"
					>
						<tr>
							{#each columns as col (col)}
								<th class="px-3 py-2 text-left font-medium text-muted-foreground">
									{col}
								</th>
							{/each}
						</tr>
					</thead>
					<tbody>
						{#each items as item, i (i)}
							<tr class="border-b border-border/40">
								{#each columns as col (col)}
									<td class="px-3 py-1.5 font-mono">
										{item[col] ? attributeToString(item[col]) : '—'}
									</td>
								{/each}
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>

	{#if nextToken}
		<div class="shrink-0 border-t border-border bg-background/40 px-3 py-2 text-[11px] text-muted-foreground">
			Result truncated. Add LIMIT or refine WHERE clause.
		</div>
	{/if}
</div>
