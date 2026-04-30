<script lang="ts">
	import { cn } from '$lib/utils';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import ThemePicker from '$lib/components/theme-picker.svelte';
	import Code from '@lucide/svelte/icons/code-2';
	import BookOpen from '@lucide/svelte/icons/book-open';
	import Search from '@lucide/svelte/icons/search';
	import Menu from '@lucide/svelte/icons/menu';
	import Keyboard from '@lucide/svelte/icons/keyboard';
	import Settings from '@lucide/svelte/icons/settings';

	interface Props {
		region?: string;
		accountId?: string;
		version?: string;
		onOpenPalette: () => void;
		onOpenHelp?: () => void;
		onOpenMobileNav?: () => void;
	}

	let {
		region = 'us-east-1',
		accountId = '000000000000',
		version = '0.1.0',
		onOpenPalette,
		onOpenHelp,
		onOpenMobileNav,
	}: Props = $props();
</script>

<header
	class={cn(
		'h-[60px] shrink-0 border-b border-border',
		'bg-gradient-to-b from-card to-background',
		'flex items-center gap-2 px-3 sm:px-4'
	)}
>
	<!-- Mobile hamburger -->
	{#if onOpenMobileNav}
		<Button
			type="button"
			variant="ghost"
			size="icon"
			class="md:hidden"
			onclick={onOpenMobileNav}
			aria-label="Open navigation"
		>
			<Menu class="size-5" />
		</Button>
	{/if}

	<!-- Brand -->
	<a
		href="/"
		class="flex items-center gap-2 rounded-md px-2 py-1 transition-colors hover:bg-muted/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
	>
		<div
			class="flex size-7 items-center justify-center rounded-md bg-gradient-to-br from-primary to-primary/60 shadow-sm shadow-primary/20"
		>
			<span class="font-mono text-sm font-bold text-primary-foreground">A</span>
		</div>
		<div class="hidden flex-col leading-none sm:flex">
			<span class="text-sm font-semibold tracking-tight">AWSim</span>
			<span class="font-mono text-[10px] text-muted-foreground">v{version}</span>
		</div>
	</a>

	<!-- Region / Account chip -->
	<div class="hidden items-center gap-1.5 lg:flex">
		<Badge variant="outline" class="gap-1 font-mono text-[11px] font-normal">
			<span class="text-muted-foreground">region</span>
			<span>{region}</span>
		</Badge>
		<Badge variant="outline" class="gap-1 font-mono text-[11px] font-normal">
			<span class="text-muted-foreground">acct</span>
			<span>{accountId}</span>
		</Badge>
	</div>

	<!-- Cmd-K search trigger (centered, takes the slack) -->
	<div class="mx-auto w-full max-w-md flex-1 px-2">
		<button
			type="button"
			onclick={onOpenPalette}
			class={cn(
				'group flex h-9 w-full items-center gap-2 rounded-md border border-border bg-background/60 px-3 text-sm',
				'text-muted-foreground transition-all duration-100',
				'hover:border-border/80 hover:bg-background hover:text-foreground',
				'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring'
			)}
			aria-label="Open command palette"
		>
			<Search class="size-3.5 shrink-0" />
			<span class="flex-1 text-left">Search services, actions...</span>
			<kbd
				class="hidden h-5 select-none items-center gap-0.5 rounded border border-border bg-muted px-1.5 font-mono text-[10px] text-muted-foreground sm:inline-flex"
			>
				<span class="text-xs">⌘</span>K
			</kbd>
		</button>
	</div>

	<!-- Right cluster -->
	<div class="flex items-center gap-1">
		<ThemePicker />

		{#if onOpenHelp}
			<Button
				type="button"
				variant="ghost"
				size="icon"
				onclick={onOpenHelp}
				aria-label="Keyboard shortcuts"
				class="hidden sm:inline-flex"
			>
				<Keyboard class="size-4" />
			</Button>
		{/if}

		<Button
			variant="ghost"
			size="icon"
			href="/settings"
			aria-label="Settings"
		>
			<Settings class="size-4" />
		</Button>

		<Button
			variant="ghost"
			size="icon"
			href="https://github.com/QaidVoid/awsim"
			target="_blank"
			rel="noopener"
			aria-label="Source repository"
		>
			<Code class="size-4" />
		</Button>

		<Button
			variant="ghost"
			size="icon"
			href="/"
			aria-label="Documentation"
		>
			<BookOpen class="size-4" />
		</Button>
	</div>
</header>
