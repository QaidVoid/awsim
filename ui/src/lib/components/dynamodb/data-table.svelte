<script lang="ts">
	import {
		attributeToString,
		attributeType,
		type AttributeValue,
		type Item,
		type KeySchemaElement
	} from '$lib/api/dynamodb';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import {
		DropdownMenu,
		DropdownMenuCheckboxItem,
		DropdownMenuContent,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import ArrowUp from '@lucide/svelte/icons/arrow-up';
	import ArrowDown from '@lucide/svelte/icons/arrow-down';
	import Funnel from '@lucide/svelte/icons/funnel';
	import Columns3 from '@lucide/svelte/icons/columns-3';

	interface Props {
		items: Item[];
		/** Key attributes pinned first with PK/SK badges. */
		keySchema?: KeySchemaElement[];
		/** Resets sort, filters, visibility, and widths when it changes. */
		resetKey?: string;
		onRowClick?: (item: Item) => void;
		/** Renders a sticky delete column when provided. */
		onDelete?: (item: Item) => void;
	}

	let { items, keySchema = [], resetKey = '', onRowClick, onDelete }: Props = $props();

	// Client-side view state: sorting, per-column filters, visibility,
	// and drag-resized widths. All apply to the rows passed in; server
	// pagination is the caller's concern.
	let sortCol = $state<string | null>(null);
	let sortDir = $state<'asc' | 'desc'>('asc');
	let showFilters = $state(false);
	let filters = $state<Record<string, string>>({});
	let hiddenCols = $state<Set<string>>(new Set());
	let colWidths = $state<Record<string, number>>({});
	let headerRow = $state<HTMLTableRowElement | null>(null);
	let userResized = $state(false);

	$effect(() => {
		void resetKey;
		sortCol = null;
		showFilters = false;
		filters = {};
		hiddenCols = new Set();
		colWidths = {};
		userResized = false;
	});

	let columns = $derived.by(() => {
		const keyCols = new Set<string>();
		const ordered: string[] = [];
		for (const k of keySchema) {
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

	// The table switches to fixed layout once any width is set, which
	// would collapse every unsized column to an even share. Snapshotting
	// every rendered width first means only deliberate changes move
	// columns. Also used when the filter row opens, so filtering rows
	// cannot reflow the auto-layout widths under the cursor.
	function freezeWidths() {
		if (Object.keys(colWidths).length > 0 || !headerRow) return;
		const ths = Array.from(headerRow.children) as HTMLElement[];
		const snapshot: Record<string, number> = {};
		visibleColumns.forEach((c, i) => {
			const el = ths[i];
			if (el) snapshot[c] = Math.round(el.getBoundingClientRect().width);
		});
		colWidths = snapshot;
	}

	function toggleFilters() {
		if (!showFilters) {
			freezeWidths();
			showFilters = true;
		} else {
			showFilters = false;
			// Return to auto layout unless the user resized something or
			// filters are still narrowing the rows.
			if (!userResized && activeFilterCount === 0) colWidths = {};
		}
	}

	function startResize(col: string, e: PointerEvent) {
		e.preventDefault();
		e.stopPropagation();
		const th = (e.target as HTMLElement).closest('th');
		freezeWidths();
		userResized = true;
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

	function keyBadge(col: string): string | null {
		const k = keySchema.find((s) => s.attributeName === col);
		if (!k) return null;
		return k.keyType === 'HASH' ? 'PK' : 'SK';
	}

	function valueDisplay(v: AttributeValue | undefined): string {
		if (v === undefined) return '—';
		const s = attributeToString(v);
		// Hard-cap the cell text so one huge attribute can't blow out the
		// table; the full value is available on the row click target.
		return s.length > 300 ? s.slice(0, 300) + ' ...' : s;
	}

	function valueType(v: AttributeValue | undefined): string {
		if (v === undefined) return '';
		return attributeType(v);
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div
		class="flex shrink-0 items-center justify-end gap-1.5 border-b border-border/60 bg-background/40 px-2 py-1"
	>
		{#if activeFilterCount > 0}
			<span class="mr-auto px-1 text-[11px] text-muted-foreground">
				{displayItems.length} of {items.length} shown
			</span>
		{/if}
		<Button
			variant={showFilters || activeFilterCount > 0 ? 'secondary' : 'ghost'}
			size="icon-sm"
			onclick={toggleFilters}
			aria-label="Toggle column filters"
			title="Filter columns (matches the fetched rows)"
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
	</div>

	<div class="min-h-0 flex-1 overflow-auto">
		<table class="w-full text-xs" class:table-fixed={Object.keys(colWidths).length > 0}>
			<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
				<tr bind:this={headerRow}>
					{#each visibleColumns as col (col)}
						<th
							class="group/th relative px-3 py-2 text-left font-medium text-muted-foreground"
							style={colStyle(col)}
						>
							<button
								type="button"
								class="flex max-w-full items-center gap-1 hover:text-foreground"
								onclick={() => toggleSort(col)}
								title="Sort by {col} (sorts the fetched rows)"
							>
								<span class="truncate">{col}</span>
								{#if keyBadge(col)}
									<span class="shrink-0 text-[10px] text-primary">{keyBadge(col)}</span>
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
					{#if onDelete}
						<th
							class="sticky right-0 z-20 w-10 border-l border-border bg-background/95 backdrop-blur-sm"
						></th>
					{/if}
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
						{#if onDelete}
							<td
								class="sticky right-0 z-20 border-l border-border bg-background/95 backdrop-blur-sm"
							></td>
						{/if}
					</tr>
				{/if}
			</thead>
			<tbody>
				{#each displayItems as item, i (i)}
					<tr
						class="group border-b border-border/40 {onRowClick
							? 'cursor-pointer hover:bg-muted/40'
							: ''}"
						onclick={() => onRowClick?.(item)}
					>
						{#each visibleColumns as col (col)}
							<td class="px-3 py-1.5 align-top" style={colStyle(col)}>
								<div class="flex items-baseline gap-1">
									<span
										class="block truncate font-mono {colWidths[col] ? '' : 'max-w-[24rem]'}"
									>
										{valueDisplay(item[col])}
									</span>
									{#if item[col]}
										<Badge variant="outline" class="shrink-0 align-baseline text-[9px]">
											{valueType(item[col])}
										</Badge>
									{/if}
								</div>
							</td>
						{/each}
						{#if onDelete}
							<td
								class="sticky right-0 border-l border-border bg-background/95 px-2 align-top backdrop-blur-sm group-hover:bg-muted/95"
							>
								<Button
									variant="ghost"
									size="icon-xs"
									aria-label="Delete item"
									onclick={(e: MouseEvent) => {
										e.stopPropagation();
										onDelete(item);
									}}
								>
									<Trash2 class="size-3 text-destructive" />
								</Button>
							</td>
						{/if}
					</tr>
				{/each}
				{#if displayItems.length === 0 && items.length > 0}
					<tr>
						<td
							class="px-3 py-6 text-center text-muted-foreground"
							colspan={visibleColumns.length + (onDelete ? 1 : 0)}
						>
							No rows match the filters.
						</td>
					</tr>
				{/if}
			</tbody>
		</table>
	</div>
</div>
