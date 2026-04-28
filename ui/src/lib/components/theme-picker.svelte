<script lang="ts">
	/**
	 * Topbar dropdown that lets the user pick any theme variant. Each
	 * row shows the variant's name plus a 3-swatch preview (background,
	 * foreground, accent) so the choice reads at a glance.
	 */
	import { Button } from '$lib/components/ui/button';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuItem,
		DropdownMenuLabel,
		DropdownMenuSeparator,
		DropdownMenuTrigger,
	} from '$lib/components/ui/dropdown-menu';
	import { THEMES, theme, type Theme } from '$lib/theme.svelte';
	import Sun from '@lucide/svelte/icons/sun';
	import Moon from '@lucide/svelte/icons/moon';
	import Check from '@lucide/svelte/icons/check';

	const currentMeta = $derived(THEMES.find((t) => t.id === theme.current) ?? THEMES[0]);

	function pick(id: Theme) {
		theme.set(id);
	}
</script>

<DropdownMenu>
	<DropdownMenuTrigger>
		{#snippet child({ props })}
			<Button
				{...props}
				type="button"
				variant="ghost"
				size="icon"
				aria-label="Pick theme — current: {currentMeta.label}"
				class="transition-all duration-100"
			>
				{#if theme.isDark}
					<Moon class="size-4" />
				{:else}
					<Sun class="size-4" />
				{/if}
			</Button>
		{/snippet}
	</DropdownMenuTrigger>

	<DropdownMenuContent align="end" class="w-[220px]">
		<DropdownMenuLabel class="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
			Theme
		</DropdownMenuLabel>
		{#each THEMES as t (t.id)}
			{@const isActive = t.id === theme.current}
			<DropdownMenuItem onSelect={() => pick(t.id)} class="flex items-center gap-2">
				<div class="flex h-4 shrink-0 items-center gap-0.5 rounded-sm border border-border/60 p-0.5">
					<span class="size-3 rounded-sm" style:background={t.swatch.bg}></span>
					<span class="size-3 rounded-sm" style:background={t.swatch.fg}></span>
					<span class="size-3 rounded-sm" style:background={t.swatch.accent}></span>
				</div>
				<span class="flex-1 text-sm">{t.label}</span>
				{#if isActive}
					<Check class="size-3.5 text-primary" />
				{/if}
			</DropdownMenuItem>
		{/each}
		<DropdownMenuSeparator />
		<DropdownMenuItem onSelect={() => theme.toggle()} class="text-xs text-muted-foreground">
			<span>Toggle dark / light</span>
			<span class="ml-auto font-mono text-[10px]">t</span>
		</DropdownMenuItem>
	</DropdownMenuContent>
</DropdownMenu>
