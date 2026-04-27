<script lang="ts" generics="T">
	import type { Snippet } from 'svelte';
	import { cn } from '$lib/utils';

	interface Column<Row> {
		key: string;
		label: string;
		width?: string;
		align?: 'left' | 'right' | 'center';
		mono?: boolean;
		cell?: Snippet<[Row]>;
	}

	interface Props {
		rows: T[];
		columns: Column<T>[];
		empty?: Snippet;
		rowKey?: (row: T, idx: number) => string;
		onRowClick?: (row: T) => void;
		dense?: boolean;
		class?: string;
	}

	let {
		rows,
		columns,
		empty,
		rowKey = (_r: T, i: number) => String(i),
		onRowClick,
		dense = false,
		class: className
	}: Props = $props();
</script>

<div class={cn('flex h-full min-h-0 flex-col overflow-hidden', className)}>
	<div class="min-h-0 flex-1 overflow-auto">
		<table class="w-full border-collapse text-sm">
			<thead
				class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm"
			>
				<tr>
					{#each columns as col (col.key)}
						<th
							class={cn(
								'px-4 text-left font-medium text-muted-foreground',
								dense ? 'py-2' : 'py-3',
								col.align === 'right' && 'text-right',
								col.align === 'center' && 'text-center'
							)}
							style={col.width ? `width: ${col.width}` : undefined}
						>
							{col.label}
						</th>
					{/each}
				</tr>
			</thead>
			<tbody>
				{#each rows as row, idx (rowKey(row, idx))}
					<tr
						class={cn(
							'border-b border-border/40 transition-colors',
							onRowClick && 'cursor-pointer hover:bg-muted/40'
						)}
						onclick={onRowClick ? () => onRowClick(row) : undefined}
					>
						{#each columns as col (col.key)}
							<td
								class={cn(
									'px-4',
									dense ? 'py-1.5' : 'py-2.5',
									col.align === 'right' && 'text-right',
									col.align === 'center' && 'text-center',
									col.mono && 'font-mono text-xs'
								)}
							>
								{#if col.cell}
									{@render col.cell(row)}
								{:else}
									{(row as Record<string, unknown>)[col.key] ?? ''}
								{/if}
							</td>
						{/each}
					</tr>
				{:else}
					<tr>
						<td colspan={columns.length} class="px-4 py-12 text-center text-muted-foreground">
							{#if empty}
								{@render empty()}
							{:else}
								No data
							{/if}
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
</div>
