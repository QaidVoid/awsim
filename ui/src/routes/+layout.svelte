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
	import InspectDrawer from '$lib/components/inspect-drawer.svelte';
	import { fetchConfig } from '$lib/api';
	import { fetchRecentRequestIds } from '$lib/api/requests';
	import { auth } from '$lib/auth-state.svelte';
	import { credentials, installFetchSigner } from '$lib/credentials.svelte';
	import { ENDPOINT } from '$lib/aws';
	import { route } from '$lib/url';
	import { recent } from '$lib/recent.svelte';
	import { shortcuts } from '$lib/shortcuts.svelte';
	import { dashboardState } from '$lib/dashboard-state.svelte';
	import { inspectState } from '$lib/inspect-state.svelte';
	import { toast } from 'svelte-sonner';
	import { theme } from '$lib/theme.svelte';

	let { children } = $props();

	// Sidebar collapse persistence
	const COLLAPSE_KEY = 'awsim-sidebar-collapsed';
	let sidebarCollapsed = $state(false);
	let mobileOpen = $state(false);
	let paletteOpen = $state(false);
	let helpOpen = $state(false);
	let config = $state<{ region?: string; accountId?: string } | null>(null);

	/** Routes that render bare (no sidebar / topbar) and shouldn't redirect.
	 * Handles a trailing slash and the SvelteKit base prefix
	 * (`/_awsim/ui/login/`) by normalizing first. */
	function isAuthRoute(path: string | undefined): boolean {
		if (!path) return false;
		const trimmed = path.replace(/\/+$/, '');
		return trimmed.endsWith('/login') || trimmed.endsWith('/setup');
	}

	const pathname = $derived(page.url?.pathname ?? '');
	const onAuthRoute = $derived(isAuthRoute(pathname));
	/** Block the app shell from rendering protected content while the gate is on. */
	const hideAppContent = $derived(!onAuthRoute && (!auth.loaded || auth.blocked || auth.setupRequired));

	function registerShortcuts() {
		shortcuts.register([
			// General
			{ keys: '?', category: 'General', description: 'Show keyboard shortcuts', action: () => (helpOpen = true) },
			{ keys: '/', category: 'General', description: 'Open command palette', action: () => (paletteOpen = true) },
			{ keys: 't', category: 'General', description: 'Toggle theme', action: () => theme.toggle() },
			{ keys: '[', category: 'General', description: 'Toggle sidebar', action: () => toggleCollapse() },
			{
				keys: 'i',
				category: 'General',
				description: 'Inspect last request',
				action: () => inspectLatest(),
			},

			// Navigation (g leader)
			{ keys: 'g d', category: 'Navigation', description: 'Dashboard', action: () => goto(route('/')) },
			{ keys: 'g r', category: 'Navigation', description: 'Request log', action: () => goto(route('/logs')) },
			{ keys: 'g s', category: 'Navigation', description: 'S3', action: () => goto(route('/s3')) },
			{ keys: 'g f', category: 'Navigation', description: 'Lambda (function)', action: () => goto(route('/lambda')) },
			{ keys: 'g t', category: 'Navigation', description: 'DynamoDB (table)', action: () => goto(route('/dynamodb')) },
			{ keys: 'g i', category: 'Navigation', description: 'IAM', action: () => goto(route('/iam')) },
			{ keys: 'g q', category: 'Navigation', description: 'SQS (queue)', action: () => goto(route('/sqs')) },
			{ keys: 'g n', category: 'Navigation', description: 'SNS (notify)', action: () => goto(route('/sns')) },
			{ keys: 'g k', category: 'Navigation', description: 'KMS (key)', action: () => goto(route('/kms')) },
			{ keys: 'g e', category: 'Navigation', description: 'EC2', action: () => goto(route('/ec2')) },
			{ keys: 'g c', category: 'Navigation', description: 'Cognito', action: () => goto(route('/cognito')) },
			{ keys: 'g m', category: 'Navigation', description: 'Metrics', action: () => goto(route('/monitoring')) },
			{ keys: 'g x', category: 'Navigation', description: 'CloudTrail', action: () => goto(route('/cloudtrail')) },
			{ keys: 'g w', category: 'Navigation', description: 'CloudWatch logs', action: () => goto(route('/cloudwatch')) },
			{ keys: 'g b', category: 'Navigation', description: 'Bedrock', action: () => goto(route('/bedrock')) },
			{ keys: 'g p', category: 'Navigation', description: 'API Gateway', action: () => goto(route('/apigateway')) },
			{ keys: 'g ,', category: 'Navigation', description: 'Settings', action: () => goto(route('/settings')) },
		]);
	}

	onMount(() => {
		try {
			sidebarCollapsed = localStorage.getItem(COLLAPSE_KEY) === '1';
		} catch {
			/* ignore */
		}
		// Install the SigV4 interceptor before any API calls fire.
		// The interceptor rewrites every outbound AWS request to the
		// gateway with a real signature, so the server can run policy
		// evaluation against the operator's IAM principal. Falls back
		// to the admin key when no operator session is present.
		installFetchSigner(ENDPOINT);
		// Skip protected admin probes on the bare auth pages: they sit
		// behind the operator-auth middleware and would 503 / 401
		// before sign-in, polluting the console.
		const initialPath = window.location.pathname;
		if (!isAuthRoute(initialPath)) {
			fetchConfig()
				.then((c) => (config = c))
				.catch(() => {
					/* leave defaults */
				});
			// Pick up the operator's IAM credentials so subsequent
			// signed requests carry their access key (and therefore
			// surface the right principal to policy evaluation).
			void credentials.refresh();
		}

		// Probe whoami to populate the session. The response always
		// 200s with { auth_required, setup_required, principal } so a
		// loginless build looks distinct from an enabled-but-not-yet-
		// signed-in build. We redirect to /setup or /login only when
		// the gate actually applies.
		auth.refresh()
			.then(() => {
				const path = page.url.pathname;
				if (auth.setupRequired && !isAuthRoute(path)) {
					goto(route('/setup'));
				} else if (auth.blocked && !isAuthRoute(path)) {
					goto(route('/login'));
				}
			})
			.catch(() => {
				/* leave loginless */
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

	async function inspectLatest() {
		// Prefer the SSE buffer (we have full event metadata there) — fall
		// back to fetching the recent-ids list when the user is on a page
		// that doesn't subscribe to dashboardState.
		const last = dashboardState.events[0];
		if (last) {
			inspectState.show(last.id, last);
			return;
		}
		try {
			const id = (await fetchRecentRequestIds())[0];
			if (!id) {
				toast.info('No recent requests to inspect — hit any endpoint first.');
				return;
			}
			inspectState.show(id, null);
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load recent requests');
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
	{#if onAuthRoute}
		<!-- Bare shell for /login and /setup. No sidebar, no topbar,
			 nothing the user could click to bypass the gate. -->
		<div class="min-h-screen w-screen bg-background text-foreground">
			{@render children()}
		</div>
	{:else}
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

				<!-- Main content — pages own their scroll containment via ServicePage.
					 Hidden while the auth gate is still resolving or actively
					 blocking so an unauthenticated user can never see the
					 protected pages even for a frame. -->
				<main class="flex min-h-0 flex-1 flex-col overflow-hidden">
					{#if hideAppContent}
						<div class="flex flex-1 items-center justify-center text-xs text-muted-foreground">
							{auth.loaded ? 'Redirecting...' : 'Loading...'}
						</div>
					{:else}
						{@render children()}
					{/if}
				</main>

				<!-- Optional context drawer slot — hidden by default, future-use. -->
				<aside class="hidden w-[320px] shrink-0 border-l border-border bg-card xl:hidden"></aside>
			</div>
		</div>
	{/if}

	<CommandPalette bind:open={paletteOpen} />
	<KeyboardHelp bind:open={helpOpen} />
	<InspectDrawer />
	<LeaderHint />
	<Toaster />
</TooltipProvider>
