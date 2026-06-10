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
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { EmptyState } from '$lib/components/service';
	import {
		DropdownMenu,
		DropdownMenuCheckboxItem,
		DropdownMenuContent,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Inbox from '@lucide/svelte/icons/inbox';
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import ArrowUp from '@lucide/svelte/icons/arrow-up';
	import ArrowDown from '@lucide/svelte/icons/arrow-down';
	import Funnel from '@lucide/svelte/icons/funnel';
	import Columns3 from '@lucide/svelte/icons/columns-3';

	interface Props {
		detail: TableDetail;
		onEdit: (item: Item | null) => void;
	}

	let { detail, onEdit }: Props = $props();

	type SkOp = 'EQ' | 'LT' | 'LE' | 'GT' | 'GE' | 'BEGINS_WITH';
	const SK_OP_LABELS: Record<SkOp, string> = {
		EQ: '=',
		LT: '<',
		LE: '<=',
		GT: '>',
		GE: '>=',
		BEGINS_WITH: 'begins_with'
	};

	let mode = $state<'scan' | 'query'>('scan');
	let selectedIndexName = $state<string>('');
	let pkValue = $state('');
	let pkType = $state<ScalarType>('S');
	let skValue = $state('');
	let skType = $state<ScalarType>('S');
	let skOp = $state<SkOp>('EQ');
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
		const keyCols = new Set<string>();
		const ordered: string[] = [];
		for (const k of detail.keySchema) {
			if (!keyCols.has(k.attributeName)) {
				keyCols.add(k.attributeName);
				ordered.push(k.attributeName);
			}
		}
		// Non-key attributes sort alphabetically: the wire order of item
		// maps is serialization-dependent and changes between refreshes.
		const rest = new Set<string>();
		for (const item of items) {
			for (const k of Object.keys(item)) {
				if (!keyCols.has(k)) rest.add(k);
			}
		}
		ordered.push(...[...rest].sort((a, b) => a.localeCompare(b)));
		return ordered;
	});

	// Client-side view state: sorting, per-column filters, visibility,
	// and drag-resized widths. All apply to the fetched page only; the
	// server cursor pagination above is unaffected.
	let sortCol = $state<string | null>(null);
	let sortDir = $state<'asc' | 'desc'>('asc');
	let showFilters = $state(false);
	let filters = $state<Record<string, string>>({});
	let hiddenCols = $state<Set<string>>(new Set());
	let colWidths = $state<Record<string, number>>({});

	$effect(() => {
		if (detail.name) {
			sortCol = null;
			filters = {};
			hiddenCols = new Set();
			colWidths = {};
			void reset();
		}
	});

	let visibleColumns = $derived(columns.filter((c) => !hiddenCols.has(c)));
	let activeFilterCount = $derived(
		Object.values(filters).filter((v) => v.trim().length > 0).length
	);

	/** Sortable scalar for a cell: numbers compare numerically. */
	function sortValue(item: Item, col: string): string | number | null {
		const v = item[col];
		if (v === undefined) return null;
		if ('N' in v) {
			const n = Number(v.N);
			if (!Number.isNaN(n)) return n;
		}
		return attributeToString(v).toLowerCase();
	}

	let displayItems = $derived.by(() => {
		let rows = items;
		const active = Object.entries(filters).filter(([, v]) => v.trim().length > 0);
		if (active.length > 0) {
			rows = rows.filter((item) =>
				active.every(([col, needle]) => {
					const v = item[col];
					if (v === undefined) return false;
					return attributeToString(v).toLowerCase().includes(needle.trim().toLowerCase());
				})
			);
		}
		const col = sortCol;
		if (col) {
			const dir = sortDir === 'asc' ? 1 : -1;
			rows = [...rows].sort((a, b) => {
				const av = sortValue(a, col);
				const bv = sortValue(b, col);
				// Missing attributes sort last in either direction.
				if (av === null && bv === null) return 0;
				if (av === null) return 1;
				if (bv === null) return -1;
				if (typeof av === 'number' && typeof bv === 'number') return (av - bv) * dir;
				return String(av).localeCompare(String(bv)) * dir;
			});
		}
		return rows;
	});

	function toggleSort(col: string) {
		if (sortCol !== col) {
			sortCol = col;
			sortDir = 'asc';
		} else if (sortDir === 'asc') {
			sortDir = 'desc';
		} else {
			sortCol = null;
		}
	}

	function toggleColumn(col: string, visible: boolean) {
		const next = new Set(hiddenCols);
		if (visible) next.delete(col);
		else next.add(col);
		hiddenCols = next;
	}

	const MIN_COL_WIDTH = 80;

	function startResize(col: string, e: PointerEvent) {
		e.preventDefault();
		e.stopPropagation();
		const th = (e.target as HTMLElement).closest('th');
		// The table switches to fixed layout once any width is set, which
		// would collapse every unsized column to an even share. Snapshot
		// all rendered widths on the first resize so only the dragged
		// column moves.
		if (th?.parentElement && Object.keys(colWidths).length === 0) {
			const ths = Array.from(th.parentElement.children) as HTMLElement[];
			const snapshot: Record<string, number> = {};
			visibleColumns.forEach((c, i) => {
				const el = ths[i];
				if (el) snapshot[c] = Math.round(el.getBoundingClientRect().width);
			});
			colWidths = snapshot;
		}
		const startWidth = colWidths[col] ?? th?.getBoundingClientRect().width ?? 160;
		const startX = e.clientX;
		const onMove = (ev: PointerEvent) => {
			colWidths = {
				...colWidths,
				[col]: Math.max(MIN_COL_WIDTH, Math.round(startWidth + ev.clientX - startX))
			};
		};
		const onUp = () => {
			window.removeEventListener('pointermove', onMove);
			window.removeEventListener('pointerup', onUp);
		};
		window.addEventListener('pointermove', onMove);
		window.addEventListener('pointerup', onUp);
	}

	function colStyle(col: string): string {
		const w = colWidths[col];
		return w ? `width: ${w}px; min-width: ${w}px; max-width: ${w}px;` : '';
	}

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
		const s = attributeToString(v);
		// Hard-cap the cell text so one huge attribute can't blow out the
		// table; the full value is in the item editor (row click).
		return s.length > 300 ? s.slice(0, 300) + ' ...' : s;
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
				<Button
					variant={showFilters || activeFilterCount > 0 ? 'secondary' : 'ghost'}
					size="icon-sm"
					onclick={() => (showFilters = !showFilters)}
					aria-label="Toggle column filters"
					title="Filter columns (matches the fetched page)"
				>
					<Funnel class="size-3.5" />
				</Button>
				<DropdownMenu>
					<DropdownMenuTrigger>
						{#snippet child({ props })}
							<Button
								{...props}
								variant={hiddenCols.size > 0 ? 'secondary' : 'ghost'}
								size="icon-sm"
								aria-label="Choose visible columns"
								title="Columns"
							>
								<Columns3 class="size-3.5" />
							</Button>
						{/snippet}
					</DropdownMenuTrigger>
					<DropdownMenuContent align="end" class="max-h-72 w-auto min-w-44 max-w-80 overflow-y-auto">
						{#each columns as col (col)}
							<DropdownMenuCheckboxItem
								checked={!hiddenCols.has(col)}
								onCheckedChange={(v) => toggleColumn(col, v)}
								closeOnSelect={false}
							>
								<span class="block truncate font-mono text-xs" title={col}>{col}</span>
							</DropdownMenuCheckboxItem>
						{/each}
					</DropdownMenuContent>
				</DropdownMenu>
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
					<Select type="single" bind:value={selectedIndexName}>
						<SelectTrigger
							aria-label="Index to query"
							size="sm"
							class="h-7 w-[180px] text-xs"
						>
							{selectedIndexName || 'Table (default)'}
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="" label="Table (default)">Table (default)</SelectItem>
							{#each detail.globalSecondaryIndexes as gsi (gsi.indexName)}
								<SelectItem value={gsi.indexName} label={gsi.indexName}
									>{gsi.indexName}</SelectItem
								>
							{/each}
						</SelectContent>
					</Select>
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
						<Select
							type="single"
							value={pkType}
							onValueChange={(v) => (pkType = v as ScalarType)}
						>
							<SelectTrigger
								aria-label="Partition key type"
								size="sm"
								class="w-16 text-xs"
							>
								{pkType}
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="S" label="S">S</SelectItem>
								<SelectItem value="N" label="N">N</SelectItem>
								<SelectItem value="B" label="B">B</SelectItem>
							</SelectContent>
						</Select>
					</div>
				</div>

				{#if querySkName}
					<div class="flex flex-col gap-1">
						<Label for="dq-sk-value" class="text-[11px]">{querySkName}</Label>
						<div class="flex gap-1">
							<Select
								type="single"
								value={skOp}
								onValueChange={(v) => (skOp = v as SkOp)}
							>
								<SelectTrigger
									aria-label="Sort operator"
									size="sm"
									class="w-32 text-xs"
								>
									{SK_OP_LABELS[skOp]}
								</SelectTrigger>
								<SelectContent>
									<SelectItem value="EQ" label="=">=</SelectItem>
									<SelectItem value="LT" label="<">&lt;</SelectItem>
									<SelectItem value="LE" label="<=">&lt;=</SelectItem>
									<SelectItem value="GT" label=">">&gt;</SelectItem>
									<SelectItem value="GE" label=">=">&gt;=</SelectItem>
									<SelectItem value="BEGINS_WITH" label="begins_with"
										>begins_with</SelectItem
									>
								</SelectContent>
							</Select>
							<Input
								id="dq-sk-value"
								bind:value={skValue}
								placeholder="value"
								class="h-8 text-xs"
							/>
							<Select
								type="single"
								value={skType}
								onValueChange={(v) => (skType = v as ScalarType)}
							>
								<SelectTrigger
									aria-label="Sort key type"
									size="sm"
									class="w-16 text-xs"
								>
									{skType}
								</SelectTrigger>
								<SelectContent>
									<SelectItem value="S" label="S">S</SelectItem>
									<SelectItem value="N" label="N">N</SelectItem>
									<SelectItem value="B" label="B">B</SelectItem>
								</SelectContent>
							</Select>
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
					{#if activeFilterCount > 0}
						{displayItems.length} of {items.length} shown ·
					{/if}
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
				<table class="w-full text-xs" class:table-fixed={Object.keys(colWidths).length > 0}>
					<thead
						class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm"
					>
						<tr>
							{#each visibleColumns as col (col)}
								<th
									class="group/th relative px-3 py-2 text-left font-medium text-muted-foreground"
									style={colStyle(col)}
								>
									<button
										type="button"
										class="flex max-w-full items-center gap-1 hover:text-foreground"
										onclick={() => toggleSort(col)}
										title="Sort by {col} (sorts the fetched page)"
									>
										<span class="truncate">{col}</span>
										{#if detail.keySchema.find((k) => k.attributeName === col)}
											<span class="shrink-0 text-[10px] text-primary">
												{detail.keySchema.find((k) => k.attributeName === col)
													?.keyType === 'HASH'
													? 'PK'
													: 'SK'}
											</span>
										{/if}
										{#if sortCol === col}
											{#if sortDir === 'asc'}
												<ArrowUp class="size-3 shrink-0" />
											{:else}
												<ArrowDown class="size-3 shrink-0" />
											{/if}
										{/if}
									</button>
									<!-- svelte-ignore a11y_no_static_element_interactions -->
									<div
										class="absolute top-0 right-0 h-full w-1.5 cursor-col-resize opacity-0 transition-opacity group-hover/th:opacity-100 hover:bg-primary/40"
										onpointerdown={(e) => startResize(col, e)}
										title="Drag to resize"
									></div>
								</th>
							{/each}
							<th
								class="sticky right-0 z-20 w-10 border-l border-border bg-background/95 backdrop-blur-sm"
							></th>
						</tr>
						{#if showFilters}
							<tr class="border-b border-border/60">
								{#each visibleColumns as col (col)}
									<td class="px-2 py-1" style={colStyle(col)}>
										<Input
											value={filters[col] ?? ''}
											oninput={(e: Event) =>
												(filters = {
													...filters,
													[col]: (e.currentTarget as HTMLInputElement).value
												})}
											placeholder="filter"
											class="h-6 px-1.5 font-mono text-[11px]"
										/>
									</td>
								{/each}
								<td
									class="sticky right-0 z-20 border-l border-border bg-background/95 backdrop-blur-sm"
								></td>
							</tr>
						{/if}
					</thead>
					<tbody>
						{#each displayItems as item, i (i)}
							<tr
								class="group cursor-pointer border-b border-border/40 hover:bg-muted/40"
								onclick={() => onEdit(item)}
							>
								{#each visibleColumns as col (col)}
									<td class="px-3 py-1.5 align-top" style={colStyle(col)}>
										<div class="flex items-baseline gap-1">
											<span
												class="block truncate font-mono {colWidths[col]
													? ''
													: 'max-w-[24rem]'}"
											>
												{valueDisplay(item[col])}
											</span>
											{#if item[col]}
												<Badge
													variant="outline"
													class="shrink-0 align-baseline text-[9px]"
												>
													{valueType(item[col])}
												</Badge>
											{/if}
										</div>
									</td>
								{/each}
								<td
									class="sticky right-0 border-l border-border bg-background/95 px-2 align-top backdrop-blur-sm group-hover:bg-muted/95"
								>
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
						{#if displayItems.length === 0 && items.length > 0}
							<tr>
								<td
									class="px-3 py-6 text-center text-muted-foreground"
									colspan={visibleColumns.length + 1}
								>
									No items on this page match the filters.
								</td>
							</tr>
						{/if}
					</tbody>
				</table>
			</div>
		{/if}
	</div>
</div>
