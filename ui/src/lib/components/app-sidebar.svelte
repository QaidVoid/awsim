<script lang="ts">
	import { page } from '$app/state';
	import { cn } from '$lib/utils';
	import {
		CATEGORY_ORDER,
		servicesByCategory,
		isBasicService,
		type ServiceCategory
	} from '$lib/services-catalog';
	import { route, isActiveRoute } from '$lib/url';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import PanelLeftClose from '@lucide/svelte/icons/panel-left-close';
	import PanelLeftOpen from '@lucide/svelte/icons/panel-left-open';
	import LayoutDashboard from '@lucide/svelte/icons/layout-dashboard';
	import { Tooltip, TooltipContent, TooltipTrigger } from '$lib/components/ui/tooltip';

	interface Props {
		collapsed: boolean;
		onCollapseToggle: () => void;
		onNavigate?: () => void;
	}

	let { collapsed, onCollapseToggle, onNavigate }: Props = $props();

	const grouped = servicesByCategory();

	// Each category collapsed/expanded state lives here. Default: all open.
	let openCategories = $state<Record<ServiceCategory, boolean>>(
		Object.fromEntries(CATEGORY_ORDER.map((c) => [c, true])) as Record<ServiceCategory, boolean>
	);

	function isActive(href: string): boolean {
		return isActiveRoute(page.url?.pathname ?? '', href);
	}

	function toggleCategory(c: ServiceCategory) {
		openCategories[c] = !openCategories[c];
	}
</script>

<div
	class={cn(
		'group/sidebar flex h-full flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground',
		collapsed ? 'w-[60px]' : 'w-[240px]'
	)}
	data-collapsed={collapsed}
>
	<!-- Home / Dashboard quick link sits at the very top of the nav, visually
		 separated from the category groups. -->
	<div class="px-2 py-2">
		<a
			href={route('/')}
			onclick={onNavigate}
			class={cn(
				'flex items-center gap-2 rounded-md px-2 py-1.5 text-sm transition-all duration-100',
				'hover:bg-sidebar-accent hover:text-sidebar-accent-foreground',
				'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
				page.url?.pathname === route('/')
					? 'bg-primary/10 font-medium text-primary'
					: 'text-muted-foreground'
			)}
			aria-label="Dashboard"
		>
			<LayoutDashboard class="size-4 shrink-0" />
			{#if !collapsed}
				<span class="truncate">Dashboard</span>
			{/if}
		</a>
	</div>

	<nav class="flex-1 overflow-y-auto px-2 pb-2" aria-label="Services">
		{#each CATEGORY_ORDER as category (category)}
			{@const items = grouped.get(category) ?? []}
			{#if items.length}
				<div class="mb-1">
					{#if !collapsed}
						<button
							type="button"
							onclick={() => toggleCategory(category)}
							class={cn(
								'group flex w-full items-center justify-between rounded px-2 py-1 text-[11px] font-medium uppercase tracking-wider',
								'text-muted-foreground hover:text-foreground transition-colors duration-100'
							)}
						>
							<span>{category}</span>
							<ChevronDown
								class={cn(
									'size-3 transition-transform duration-150',
									openCategories[category] ? 'rotate-0' : '-rotate-90'
								)}
							/>
						</button>
					{:else}
						<div
							class="my-2 mx-2 h-px bg-sidebar-border"
							aria-hidden="true"
							title={category}
						></div>
					{/if}

					{#if openCategories[category] || collapsed}
						<ul class="mt-0.5 space-y-px">
							{#each items as svc (svc.id)}
								{@const active = isActive(svc.href)}
								<li>
									{#if collapsed}
										<Tooltip delayDuration={50}>
											<TooltipTrigger>
												{#snippet child({ props })}
													<a
														{...props}
														href={route(svc.href)}
														onclick={onNavigate}
														class={cn(
															'relative flex h-8 w-full items-center justify-center rounded-md transition-all duration-100',
															'hover:bg-sidebar-accent hover:text-sidebar-accent-foreground',
															'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
															active
																? 'bg-primary/10 text-primary before:absolute before:left-0 before:top-1/2 before:h-5 before:w-[3px] before:-translate-y-1/2 before:rounded-r-full before:bg-primary'
																: 'text-muted-foreground'
														)}
														aria-label={svc.name}
													>
														<svc.icon class="size-4 shrink-0" />
													</a>
												{/snippet}
											</TooltipTrigger>
											<TooltipContent side="right" class="font-mono text-xs">
												{svc.name}{#if isBasicService(svc.id)}
													<span class="text-muted-foreground"> - read-only</span>
												{/if}
											</TooltipContent>
										</Tooltip>
									{:else}
										<a
											href={route(svc.href)}
											onclick={onNavigate}
											class={cn(
												'relative flex items-center gap-2 rounded-md px-2 py-1.5 text-sm transition-all duration-100',
												'hover:bg-sidebar-accent hover:text-sidebar-accent-foreground',
												'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
												active
													? 'bg-primary/10 font-medium text-primary before:absolute before:left-0 before:top-1/2 before:h-5 before:w-[3px] before:-translate-y-1/2 before:rounded-r-full before:bg-primary'
													: 'text-muted-foreground'
											)}
										>
											<svc.icon class="size-4 shrink-0" />
											<span class="truncate">{svc.name}</span>
											{#if isBasicService(svc.id)}
												<span
													class="ml-auto size-1.5 shrink-0 rounded-full bg-muted-foreground/40"
													title="Read-only / metadata in this UI"
													aria-label="Read-only or metadata only in this UI"
												></span>
											{/if}
										</a>
									{/if}
								</li>
							{/each}
						</ul>
					{/if}
				</div>
			{/if}
		{/each}
	</nav>

	<div class="border-t border-sidebar-border p-2">
		<button
			type="button"
			onclick={onCollapseToggle}
			class={cn(
				'flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-xs',
				'text-muted-foreground hover:text-foreground hover:bg-sidebar-accent transition-all duration-100',
				'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
				collapsed && 'justify-center'
			)}
			aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
		>
			{#if collapsed}
				<PanelLeftOpen class="size-4" />
			{:else}
				<PanelLeftClose class="size-4" />
				<span>Collapse</span>
			{/if}
		</button>
	</div>
</div>
