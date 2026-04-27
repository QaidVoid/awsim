<script lang="ts">
	/**
	 * Toolbar for the Request Log page — filter input, tabs,
	 * pause/resume, clear and the per-column visibility dropdown.
	 */
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Tabs, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuLabel,
		DropdownMenuSeparator,
		DropdownMenuTrigger,
		DropdownMenuCheckboxItem,
	} from '$lib/components/ui/dropdown-menu';
	import Pause from '@lucide/svelte/icons/pause';
	import Play from '@lucide/svelte/icons/play';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Search from '@lucide/svelte/icons/search';
	import Columns from '@lucide/svelte/icons/columns-3';
	import { dashboardState } from '$lib/dashboard-state.svelte';
	import { ALL_COLUMNS, COLUMN_LABELS, type ColumnKey, type LogTab } from './types';

	interface Props {
		tab: LogTab;
		query: string;
		visibleColumns: Record<ColumnKey, boolean>;
		onTabChange: (t: LogTab) => void;
		onQueryChange: (q: string) => void;
		onColumnToggle: (k: ColumnKey, v: boolean) => void;
	}

	let {
		tab,
		query,
		visibleColumns,
		onTabChange,
		onQueryChange,
		onColumnToggle,
	}: Props = $props();
</script>

<div class="flex w-full flex-wrap items-center gap-2">
	<div class="relative flex-1 min-w-[200px] max-w-sm">
		<Search
			class="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
		/>
		<Input
			value={query}
			oninput={(e: Event) => onQueryChange((e.target as HTMLInputElement).value)}
			placeholder="Filter by service, operation or path…"
			class="h-8 pl-7 text-xs"
			aria-label="Filter requests"
		/>
	</div>

	<Tabs value={tab} onValueChange={(v) => onTabChange(v as LogTab)}>
		<TabsList class="h-8">
			<TabsTrigger value="all" class="h-7 px-3 text-xs">All</TabsTrigger>
			<TabsTrigger value="errors" class="h-7 px-3 text-xs">Errors</TabsTrigger>
			<TabsTrigger value="slow" class="h-7 px-3 text-xs">Slow</TabsTrigger>
		</TabsList>
	</Tabs>

	<div class="ml-auto flex items-center gap-2">
		<Button
			size="sm"
			variant="outline"
			class="h-8 gap-1.5 px-2"
			onclick={() => dashboardState.togglePause()}
			title={dashboardState.paused ? 'Resume' : 'Pause'}
		>
			{#if dashboardState.paused}
				<Play class="size-3.5" /><span class="text-xs">Resume</span>
			{:else}
				<Pause class="size-3.5" /><span class="text-xs">Pause</span>
			{/if}
		</Button>
		<Button
			size="sm"
			variant="outline"
			class="h-8 gap-1.5 px-2"
			onclick={() => dashboardState.clear()}
			title="Clear"
		>
			<Trash2 class="size-3.5" /><span class="text-xs">Clear</span>
		</Button>
		<DropdownMenu>
			<DropdownMenuTrigger>
				{#snippet child({ props })}
					<Button {...props} size="sm" variant="outline" class="h-8 gap-1.5 px-2">
						<Columns class="size-3.5" /><span class="text-xs">Columns</span>
					</Button>
				{/snippet}
			</DropdownMenuTrigger>
			<DropdownMenuContent align="end" class="w-44">
				<DropdownMenuLabel class="text-xs">Visible columns</DropdownMenuLabel>
				<DropdownMenuSeparator />
				{#each ALL_COLUMNS as col (col)}
					<DropdownMenuCheckboxItem
						checked={visibleColumns[col]}
						onCheckedChange={(v) => onColumnToggle(col, v)}
					>
						{COLUMN_LABELS[col]}
					</DropdownMenuCheckboxItem>
				{/each}
			</DropdownMenuContent>
		</DropdownMenu>
	</div>
</div>
