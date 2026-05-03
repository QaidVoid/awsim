<script lang="ts">
	import { toast } from 'svelte-sonner';
	import {
		scan,
		query,
		deleteItem,
		attributeToString,
		attributeType,
		inferAttribute,
		type AttributeValue,
		type Item,
		type ScalarType,
		type TableDetail
	} from '$lib/api/dynamodb';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Inbox from '@lucide/svelte/icons/inbox';
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';

	interface Props {
		detail: TableDetail;
		onEdit: (item: Item | null) => void;
	}

	let { detail, onEdit }: Props = $props();

	let mode = $state<'scan' | 'query'>('scan');
	let selectedIndexName = $state<string>('');
	let pkValue = $state('');
	let pkType = $state<ScalarType>('S');
	let skValue = $state('');
	let skType = $state<ScalarType>('S');
	let skOp = $state<'EQ' | 'LT' | 'LE' | 'GT' | 'GE' | 'BEGINS_WITH'>('EQ');
	let limit = $state(50);

	let items = $state<Item[]>([]);
	let loading = $state(false);
	let scanned = $state(0);

	let pageStack = $state<(Item | undefined)[]>([]);
	let currentStartKey = $state<Item | undefined>(undefined);
	let lastEvaluatedKey = $state<Item | undefined>(undefined);
	let hasMore = $derived(lastEvaluatedKey !== undefined);
	let pageIndex = $derived(pageStack.length);

	let pkName = $derived(detail.keySchema.find((k) => k.keyType === 'HASH')?.attributeName ?? '');
	let skName = $derived(detail.keySchema.find((k) => k.keyType === 'RANGE')?.attributeName);

	let selectedGsi = $derived(
		selectedIndexName
			? detail.globalSecondaryIndexes.find((g) => g.indexName === selectedIndexName)
			: null
	);
	let queryPkName = $derived(
		selectedGsi?.keySchema.find((k) => k.keyType === 'HASH')?.attributeName ?? pkName
	);
	let querySkName = $derived(
		selectedGsi?.keySchema.find((k) => k.keyType === 'RANGE')?.attributeName
	);

	let columns = $derived.by(() => {
		const seen = new Set<string>();
		const ordered: string[] = [];
		for (const k of detail.keySchema) {
			if (!seen.has(k.attributeName)) {
				seen.add(k.attributeName);
				ordered.push(k.attributeName);
			}
		}
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

	$effect(() => {
		if (detail.name) {
			void reset();
		}
	});

	async function fetchScanPage(startKey: Item | undefined) {
		loading = true;
		try {
			const res = await scan({
				tableName: detail.name,
				limit,
				exclusiveStartKey: startKey
			});
			items = res.items;
			scanned = res.scannedCount;
			lastEvaluatedKey = res.lastEvaluatedKey;
			currentStartKey = startKey;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Scan failed');
		} finally {
			loading = false;
		}
	}

	async function fetchQueryPage(startKey: Item | undefined) {
		if (!queryPkName || !pkValue.trim()) {
			items = [];
			scanned = 0;
			lastEvaluatedKey = undefined;
			currentStartKey = startKey;
			return;
		}
		loading = true;
		try {
			const partitionValue = inferAttribute(pkValue, pkType);
			const sortValue = querySkName && skValue ? inferAttribute(skValue, skType) : undefined;
			const res = await query({
				tableName: detail.name,
				partitionKey: queryPkName,
				partitionValue,
				sortKey: sortValue ? querySkName : undefined,
				sortValue,
				sortOperator: sortValue ? skOp : undefined,
				indexName: selectedIndexName || undefined,
				limit
			});
			items = res.items;
			scanned = res.scannedCount;
			lastEvaluatedKey = res.lastEvaluatedKey;
			currentStartKey = startKey;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Query failed');
		} finally {
			loading = false;
		}
	}

	async function fetchPage(startKey: Item | undefined) {
		if (mode === 'scan') await fetchScanPage(startKey);
		else await fetchQueryPage(startKey);
	}

	async function runQuery() {
		if (!queryPkName || !pkValue.trim()) {
			toast.error('Partition key value required');
			return;
		}
		pageStack = [];
		currentStartKey = undefined;
		await fetchQueryPage(undefined);
	}

	async function reset() {
		pageStack = [];
		currentStartKey = undefined;
		await fetchPage(undefined);
	}

	async function nextPage() {
		if (!lastEvaluatedKey) return;
		pageStack = [...pageStack, currentStartKey];
		await fetchPage(lastEvaluatedKey);
	}

	async function prevPage() {
		if (pageStack.length === 0) return;
		const newStack = [...pageStack];
		const prevKey = newStack.pop();
		pageStack = newStack;
		await fetchPage(prevKey);
	}

	async function handleDelete(item: Item) {
		const key: Item = {};
		for (const k of detail.keySchema) {
			const v = item[k.attributeName];
			if (v !== undefined) key[k.attributeName] = v;
		}
		try {
			await deleteItem(detail.name, key);
			toast.success('Item deleted');
			items = items.filter((i) => i !== item);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}

	function valueDisplay(v: AttributeValue | undefined): string {
		if (v === undefined) return '—';
		return attributeToString(v);
	}

	function valueType(v: AttributeValue | undefined): string {
		if (v === undefined) return '';
		return attributeType(v);
	}
</script>

<div class="flex h-full min-h-0 min-w-0 flex-col">
	<div class="shrink-0 border-b border-border bg-background/40 p-3">
		<div class="mb-3 flex items-center gap-2">
			<div class="flex rounded-md border border-border p-0.5">
				<button
					type="button"
					class="rounded px-3 py-1 text-xs font-medium transition-colors {mode === 'scan'
						? 'bg-muted text-foreground'
						: 'text-muted-foreground hover:text-foreground'}"
					onclick={() => {
						mode = 'scan';
						void reset();
					}}
				>
					Scan
				</button>
				<button
					type="button"
					class="rounded px-3 py-1 text-xs font-medium transition-colors {mode === 'query'
						? 'bg-muted text-foreground'
						: 'text-muted-foreground hover:text-foreground'}"
					onclick={() => (mode = 'query')}
				>
					Query
				</button>
			</div>

			<Badge variant="secondary" class="ml-1">
				Page {pageIndex + 1}{hasMore ? '+' : ''}
			</Badge>
			<Button
				variant="ghost"
				size="icon-sm"
				onclick={prevPage}
				disabled={pageStack.length === 0 || loading}
				title="Previous page"
			>
				<ChevronLeft class="size-4" />
			</Button>
			<Button
				variant="ghost"
				size="icon-sm"
				onclick={nextPage}
				disabled={!hasMore || loading}
				title="Next page"
			>
				<ChevronRight class="size-4" />
			</Button>

			<div class="ml-auto flex items-center gap-1.5">
				<Button variant="ghost" size="icon-sm" onclick={() => void reset()} aria-label="Refresh">
					{#if loading}
						<Loader2 class="size-3.5 animate-spin" />
					{:else}
						<RefreshCw class="size-3.5" />
					{/if}
				</Button>
				<Button size="sm" onclick={() => onEdit(null)}>
					<Plus class="size-3.5" />
					Add item
				</Button>
			</div>
		</div>

		{#if mode === 'query'}
			{#if detail.globalSecondaryIndexes.length > 0}
				<div class="mb-2 flex items-center gap-2">
					<Label class="text-[11px] text-muted-foreground">Index</Label>
					<select
						bind:value={selectedIndexName}
						aria-label="Index to query"
						class="h-7 rounded-md border border-border bg-background px-1.5 text-xs"
					>
						<option value="">Table (default)</option>
						{#each detail.globalSecondaryIndexes as gsi}
							<option value={gsi.indexName}>{gsi.indexName}</option>
						{/each}
					</select>
				</div>
			{/if}
			<div class="grid grid-cols-[1fr_1fr_auto] gap-2">
				<div class="flex flex-col gap-1">
					<Label for="dq-pk-value" class="text-[11px]">
						{queryPkName || 'partition key'}
					</Label>
					<div class="flex gap-1">
						<Input
							id="dq-pk-value"
							bind:value={pkValue}
							placeholder="value"
							class="h-8 text-xs"
						/>
						<select
							bind:value={pkType}
							aria-label="Partition key type"
							class="h-8 rounded-md border border-border bg-background px-1.5 text-xs"
						>
							<option value="S">S</option>
							<option value="N">N</option>
							<option value="B">B</option>
						</select>
					</div>
				</div>

				{#if querySkName}
					<div class="flex flex-col gap-1">
						<Label for="dq-sk-value" class="text-[11px]">{querySkName}</Label>
						<div class="flex gap-1">
							<select
								bind:value={skOp}
								aria-label="Sort operator"
								class="h-8 rounded-md border border-border bg-background px-1.5 text-xs"
							>
								<option value="EQ">=</option>
								<option value="LT">&lt;</option>
								<option value="LE">&lt;=</option>
								<option value="GT">&gt;</option>
								<option value="GE">&gt;=</option>
								<option value="BEGINS_WITH">begins_with</option>
							</select>
							<Input
								id="dq-sk-value"
								bind:value={skValue}
								placeholder="value"
								class="h-8 text-xs"
							/>
							<select
								bind:value={skType}
								aria-label="Sort key type"
								class="h-8 rounded-md border border-border bg-background px-1.5 text-xs"
							>
								<option value="S">S</option>
								<option value="N">N</option>
								<option value="B">B</option>
							</select>
						</div>
					</div>
				{:else}
					<div></div>
				{/if}

				<div class="flex flex-col gap-1">
					<Label for="dq-limit" class="text-[11px]">Limit</Label>
					<div class="flex items-center gap-2">
						<Input
							id="dq-limit"
							type="number"
							bind:value={limit}
							min={1}
							max={1000}
							class="h-8 w-20 text-xs"
						/>
						<Button size="sm" onclick={runQuery} disabled={loading}
							>Run query</Button
						>
					</div>
				</div>
			</div>
		{:else}
			<div class="flex items-center gap-2">
				<Label for="dq-scan-limit" class="text-[11px] text-muted-foreground">Limit</Label>
				<Input
					id="dq-scan-limit"
					type="number"
					bind:value={limit}
					min={1}
					max={1000}
					class="h-8 w-24 text-xs"
				/>
				<Button size="sm" onclick={() => void reset()} disabled={loading}>Run scan</Button>
				<span class="ml-auto text-[11px] text-muted-foreground">
					{items.length} returned · {scanned} scanned
				</span>
			</div>
		{/if}
	</div>

	<div class="min-h-0 flex-1 overflow-hidden">
		{#if items.length === 0 && !loading}
			<div class="flex h-full items-center justify-center p-6">
				<EmptyState
					icon={Inbox}
					title="No items"
					description="Add an item or adjust the query."
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
									{#if detail.keySchema.find((k) => k.attributeName === col)}
										<span class="ml-1 text-[10px] text-primary">
											{detail.keySchema.find((k) => k.attributeName === col)
												?.keyType === 'HASH'
												? 'PK'
												: 'SK'}
										</span>
									{/if}
								</th>
							{/each}
							<th class="w-10"></th>
						</tr>
					</thead>
					<tbody>
						{#each items as item, i (i)}
							<tr
								class="cursor-pointer border-b border-border/40 hover:bg-muted/40"
								onclick={() => onEdit(item)}
							>
								{#each columns as col (col)}
									<td class="px-3 py-1.5 align-top">
										<span class="font-mono">{valueDisplay(item[col])}</span>
										{#if item[col]}
											<Badge variant="outline" class="ml-1 align-baseline text-[9px]">
												{valueType(item[col])}
											</Badge>
										{/if}
									</td>
								{/each}
								<td class="px-2 align-top">
									<Button
										variant="ghost"
										size="icon-xs"
										aria-label="Delete item"
										onclick={(e: MouseEvent) => {
											e.stopPropagation();
											void handleDelete(item);
										}}
									>
										<Trash2 class="size-3 text-destructive" />
									</Button>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>
</div>
