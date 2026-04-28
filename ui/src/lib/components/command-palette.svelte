<script lang="ts">
	import { goto } from '$app/navigation';
	import {
		CommandDialog,
		CommandEmpty,
		CommandGroup,
		CommandInput,
		CommandItem,
		CommandList,
		CommandSeparator,
		CommandShortcut,
	} from '$lib/components/ui/command';
	import { CATEGORY_ORDER, SERVICES, findService } from '$lib/services-catalog';
	import { recent } from '$lib/recent.svelte';
	import { theme } from '$lib/theme.svelte';
	import { dashboardState } from '$lib/dashboard-state.svelte';
	import { inspectState } from '$lib/inspect-state.svelte';
	import { toast } from 'svelte-sonner';
	import Plus from '@lucide/svelte/icons/plus';
	import Sun from '@lucide/svelte/icons/sun';
	import Moon from '@lucide/svelte/icons/moon';
	import Clock from '@lucide/svelte/icons/clock';
	import Eye from '@lucide/svelte/icons/eye';

	interface Props {
		open: boolean;
	}

	let { open = $bindable() }: Props = $props();
	let value = $state('');

	// Quick actions surfaced in the palette. The user can extend per service
	// later — for now we cover the most common create-resource verbs.
	const QUICK_ACTIONS = [
		{ id: 'new-bucket', label: 'Create S3 bucket', href: '/s3', keywords: ['create', 'bucket', 's3', 'new'] },
		{ id: 'new-fn', label: 'Create Lambda function', href: '/lambda', keywords: ['create', 'lambda', 'function', 'new'] },
		{ id: 'new-table', label: 'Create DynamoDB table', href: '/dynamodb', keywords: ['create', 'dynamodb', 'table', 'new'] },
		{ id: 'new-queue', label: 'Create SQS queue', href: '/sqs', keywords: ['create', 'sqs', 'queue', 'new'] },
		{ id: 'new-topic', label: 'Create SNS topic', href: '/sns', keywords: ['create', 'sns', 'topic', 'new'] },
		{ id: 'new-user', label: 'Create IAM user', href: '/iam', keywords: ['create', 'iam', 'user', 'new'] },
	];

	function go(path: string) {
		open = false;
		value = '';
		recent.push(path);
		goto(path);
	}

	async function inspectLatest() {
		open = false;
		value = '';
		const last = dashboardState.events[0];
		if (last) {
			inspectState.show(last.id, last);
			return;
		}
		try {
			const res = await fetch('/_awsim/requests');
			if (!res.ok) throw new Error(String(res.status));
			const body = (await res.json()) as { ids: string[] };
			const id = body.ids?.[0];
			if (!id) {
				toast.info('No recent requests to inspect — hit any endpoint first.');
				return;
			}
			inspectState.show(id, null);
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load recent requests');
		}
	}

	function recentLabel(path: string): string {
		const svc = SERVICES
			.filter((s) => path === s.href || path.startsWith(s.href + '/'))
			.sort((a, b) => b.href.length - a.href.length)[0];
		if (svc) return svc.name + (path === svc.href ? '' : ` ${path.slice(svc.href.length)}`);
		return path;
	}
</script>

<CommandDialog
	bind:open
	bind:value
	title="Command palette"
	description="Search services, run quick actions, jump to recent pages"
>
	<CommandInput placeholder="Search services, actions, recent pages..." />
	<CommandList class="max-h-[420px]">
		<CommandEmpty>No results found.</CommandEmpty>

		{#if recent.items.length}
			<CommandGroup heading="Recent">
				{#each recent.items as path (path)}
					{@const svc = findService(
						SERVICES.find((s) => path === s.href || path.startsWith(s.href + '/'))?.id ?? ''
					)}
					<CommandItem
						value={`recent ${path}`}
						onSelect={() => go(path)}
					>
						{#if svc}
							<svc.icon class="size-4" />
						{:else}
							<Clock class="size-4" />
						{/if}
						<span>{recentLabel(path)}</span>
						<CommandShortcut>
							<span class="font-mono text-[10px]">{path}</span>
						</CommandShortcut>
					</CommandItem>
				{/each}
			</CommandGroup>
			<CommandSeparator />
		{/if}

		<CommandGroup heading="Quick actions">
			{#each QUICK_ACTIONS as action (action.id)}
				<CommandItem
					value={`${action.label} ${action.keywords.join(' ')}`}
					onSelect={() => go(action.href)}
				>
					<Plus class="size-4" />
					<span>{action.label}</span>
				</CommandItem>
			{/each}
		</CommandGroup>

		<CommandSeparator />

		<CommandGroup heading="Tools">
			<CommandItem
				value="inspect last request raw http"
				onSelect={inspectLatest}
			>
				<Eye class="size-4" />
				<span>Inspect last request</span>
				<CommandShortcut>
					<span class="font-mono text-[10px]">i</span>
				</CommandShortcut>
			</CommandItem>
		</CommandGroup>

		<CommandSeparator />

		<CommandGroup heading="Theme">
			<CommandItem
				value="toggle theme dark light mode"
				onSelect={() => {
					theme.toggle();
					open = false;
					value = '';
				}}
			>
				{#if theme.isDark}
					<Sun class="size-4" />
					<span>Switch to light mode</span>
				{:else}
					<Moon class="size-4" />
					<span>Switch to dark mode</span>
				{/if}
			</CommandItem>
		</CommandGroup>

		<CommandSeparator />

		{#each CATEGORY_ORDER as category (category)}
			{@const items = SERVICES.filter((s) => s.category === category).sort((a, b) =>
				a.name.localeCompare(b.name)
			)}
			{#if items.length}
				<CommandGroup heading={category}>
					{#each items as svc (svc.id)}
						<CommandItem
							value={`go to ${svc.name} ${svc.id} ${(svc.keywords ?? []).join(' ')}`}
							onSelect={() => go(svc.href)}
						>
							<svc.icon class="size-4" />
							<span>{svc.name}</span>
							<CommandShortcut>
								<span class="font-mono text-[10px]">{svc.href}</span>
							</CommandShortcut>
						</CommandItem>
					{/each}
				</CommandGroup>
			{/if}
		{/each}
	</CommandList>
</CommandDialog>
