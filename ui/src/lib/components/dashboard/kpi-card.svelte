<script lang="ts">
	/**
	 * KPI card — a single information-dense tile for the dashboard's
	 * top strip. Renders a label, big value, optional secondary text,
	 * a corner icon, and an optional trend chip / sparkline placeholder.
	 *
	 * Pass `loading={true}` to show a skeleton in place of the value.
	 */
	import type { Component, Snippet } from 'svelte';
	import { Card } from '$lib/components/ui/card';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { cn } from '$lib/utils';

	interface Props {
		label: string;
		value?: string | number | null;
		secondary?: string | null;
		// eslint-disable-next-line @typescript-eslint/no-explicit-any
		icon?: Component<any>;
		mono?: boolean;
		loading?: boolean;
		accent?: 'default' | 'amber' | 'emerald' | 'sky';
		trailing?: Snippet;
	}

	let {
		label,
		value,
		secondary,
		icon: Icon,
		mono = false,
		loading = false,
		accent = 'default',
		trailing,
	}: Props = $props();

	const accentClass = $derived(
		{
			default: 'text-foreground',
			amber: 'text-amber-400',
			emerald: 'text-emerald-400',
			sky: 'text-sky-400',
		}[accent]
	);
</script>

<Card class="relative gap-2 p-4">
	<div class="flex items-start justify-between gap-2">
		<div class="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
			{label}
		</div>
		{#if Icon}
			<Icon class="size-4 shrink-0 text-muted-foreground" />
		{/if}
	</div>

	<div class="flex items-baseline justify-between gap-2">
		{#if loading || value === undefined || value === null || value === ''}
			<Skeleton class="h-7 w-20" />
		{:else}
			<div
				class={cn(
					'text-2xl font-semibold leading-tight tracking-tight',
					mono && 'font-mono',
					accentClass
				)}
			>
				{value}
			</div>
		{/if}
	</div>

	<div class="flex h-5 items-center justify-between gap-2 text-xs text-muted-foreground">
		<span class="truncate">{secondary ?? '\u00a0'}</span>
		{#if trailing}
			{@render trailing()}
		{:else}
			<Skeleton class="h-3 w-12 opacity-50" />
		{/if}
	</div>
</Card>
