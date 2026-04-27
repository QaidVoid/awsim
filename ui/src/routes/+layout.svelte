<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { afterNavigate } from '$app/navigation';
	import { page } from '$app/state';
	import { Sheet, SheetContent } from '$lib/components/ui/sheet';
	import { Toaster } from '$lib/components/ui/sonner';
	import { TooltipProvider } from '$lib/components/ui/tooltip';
	import AppSidebar from '$lib/components/app-sidebar.svelte';
	import AppTopbar from '$lib/components/app-topbar.svelte';
	import CommandPalette from '$lib/components/command-palette.svelte';
	import { fetchConfig } from '$lib/api';
	import { recent } from '$lib/recent.svelte';

	let { children } = $props();

	// Sidebar collapse persistence
	const COLLAPSE_KEY = 'awsim-sidebar-collapsed';
	let sidebarCollapsed = $state(false);
	let mobileOpen = $state(false);
	let paletteOpen = $state(false);
	let config = $state<{ region?: string; accountId?: string } | null>(null);

	onMount(() => {
		try {
			sidebarCollapsed = localStorage.getItem(COLLAPSE_KEY) === '1';
		} catch {
			/* ignore */
		}
		fetchConfig()
			.then((c) => (config = c))
			.catch(() => {
				/* leave defaults */
			});

		const onKey = (e: KeyboardEvent) => {
			if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
				e.preventDefault();
				paletteOpen = !paletteOpen;
			}
		};
		window.addEventListener('keydown', onKey);
		return () => window.removeEventListener('keydown', onKey);
	});

	function toggleCollapse() {
		sidebarCollapsed = !sidebarCollapsed;
		try {
			localStorage.setItem(COLLAPSE_KEY, sidebarCollapsed ? '1' : '0');
		} catch {
			/* ignore */
		}
	}

	// Track recent paths for the command palette.
	afterNavigate(() => {
		const path = page.url?.pathname;
		if (path && path !== '/') recent.push(path);
		mobileOpen = false;
	});
</script>

<TooltipProvider delayDuration={150}>
	<div class="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
		<AppTopbar
			region={config?.region}
			accountId={config?.accountId}
			onOpenPalette={() => (paletteOpen = true)}
			onOpenMobileNav={() => (mobileOpen = true)}
		/>

		<div class="flex min-h-0 flex-1">
			<!-- Desktop sidebar -->
			<aside class="hidden h-full md:block">
				<AppSidebar
					collapsed={sidebarCollapsed}
					onCollapseToggle={toggleCollapse}
				/>
			</aside>

			<!-- Mobile sidebar via sheet -->
			<Sheet bind:open={mobileOpen}>
				<SheetContent
					side="left"
					class="w-[260px] border-sidebar-border bg-sidebar p-0"
					showCloseButton={false}
				>
					<AppSidebar
						collapsed={false}
						onCollapseToggle={() => (mobileOpen = false)}
						onNavigate={() => (mobileOpen = false)}
					/>
				</SheetContent>
			</Sheet>

			<!-- Main content — pages own their scroll containment via ServicePage -->
			<main class="flex min-h-0 flex-1 flex-col overflow-hidden">
				{@render children()}
			</main>

			<!-- Optional context drawer slot — hidden by default, future-use. -->
			<aside class="hidden w-[320px] shrink-0 border-l border-border bg-card xl:hidden"></aside>
		</div>
	</div>

	<CommandPalette bind:open={paletteOpen} />
	<Toaster />
</TooltipProvider>
