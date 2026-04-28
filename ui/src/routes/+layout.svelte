<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { afterNavigate, goto } from '$app/navigation';
	import { page } from '$app/state';
	import { Sheet, SheetContent } from '$lib/components/ui/sheet';
	import { Toaster } from '$lib/components/ui/sonner';
	import { TooltipProvider } from '$lib/components/ui/tooltip';
	import AppSidebar from '$lib/components/app-sidebar.svelte';
	import AppTopbar from '$lib/components/app-topbar.svelte';
	import CommandPalette from '$lib/components/command-palette.svelte';
	import KeyboardHelp from '$lib/components/keyboard-help.svelte';
	import LeaderHint from '$lib/components/leader-hint.svelte';
	import { fetchConfig } from '$lib/api';
	import { recent } from '$lib/recent.svelte';
	import { shortcuts } from '$lib/shortcuts.svelte';
	import { theme } from '$lib/theme.svelte';

	let { children } = $props();

	// Sidebar collapse persistence
	const COLLAPSE_KEY = 'awsim-sidebar-collapsed';
	let sidebarCollapsed = $state(false);
	let mobileOpen = $state(false);
	let paletteOpen = $state(false);
	let helpOpen = $state(false);
	let config = $state<{ region?: string; accountId?: string } | null>(null);

	function registerShortcuts() {
		shortcuts.register([
			// General
			{ keys: '?', category: 'General', description: 'Show keyboard shortcuts', action: () => (helpOpen = true) },
			{ keys: '/', category: 'General', description: 'Open command palette', action: () => (paletteOpen = true) },
			{ keys: 't', category: 'General', description: 'Toggle theme', action: () => theme.toggle() },
			{ keys: '[', category: 'General', description: 'Toggle sidebar', action: () => toggleCollapse() },

			// Navigation (g leader)
			{ keys: 'g d', category: 'Navigation', description: 'Dashboard', action: () => goto('/') },
			{ keys: 'g r', category: 'Navigation', description: 'Request log', action: () => goto('/logs') },
			{ keys: 'g s', category: 'Navigation', description: 'S3', action: () => goto('/s3') },
			{ keys: 'g f', category: 'Navigation', description: 'Lambda (function)', action: () => goto('/lambda') },
			{ keys: 'g t', category: 'Navigation', description: 'DynamoDB (table)', action: () => goto('/dynamodb') },
			{ keys: 'g i', category: 'Navigation', description: 'IAM', action: () => goto('/iam') },
			{ keys: 'g q', category: 'Navigation', description: 'SQS (queue)', action: () => goto('/sqs') },
			{ keys: 'g n', category: 'Navigation', description: 'SNS (notify)', action: () => goto('/sns') },
			{ keys: 'g k', category: 'Navigation', description: 'KMS (key)', action: () => goto('/kms') },
			{ keys: 'g e', category: 'Navigation', description: 'EC2', action: () => goto('/ec2') },
			{ keys: 'g c', category: 'Navigation', description: 'Cognito', action: () => goto('/cognito') },
			{ keys: 'g m', category: 'Navigation', description: 'Metrics', action: () => goto('/monitoring') },
			{ keys: 'g x', category: 'Navigation', description: 'CloudTrail', action: () => goto('/cloudtrail') },
			{ keys: 'g w', category: 'Navigation', description: 'CloudWatch logs', action: () => goto('/cloudwatch') },
			{ keys: 'g b', category: 'Navigation', description: 'Bedrock', action: () => goto('/bedrock') },
			{ keys: 'g p', category: 'Navigation', description: 'API Gateway', action: () => goto('/apigateway') },
		]);
	}

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

		registerShortcuts();
		shortcuts.start();

		const onKey = (e: KeyboardEvent) => {
			if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
				e.preventDefault();
				paletteOpen = !paletteOpen;
			}
		};
		window.addEventListener('keydown', onKey);
		return () => {
			window.removeEventListener('keydown', onKey);
			shortcuts.stop();
		};
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
			onOpenHelp={() => (helpOpen = true)}
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
	<KeyboardHelp bind:open={helpOpen} />
	<LeaderHint />
	<Toaster />
</TooltipProvider>
