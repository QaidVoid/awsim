<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { listAppClients, deleteAppClient, type CognitoAppClient } from '$lib/api/cognito';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import ClientDetail from './client-detail.svelte';
	import CreateClientDialog from './create-client-dialog.svelte';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';

	interface Props {
		poolId: string;
	}

	let { poolId }: Props = $props();

	let clients = $state<CognitoAppClient[]>([]);
	let loading = $state(false);
	let pageStack = $state<(string | undefined)[]>([]);
	let currentToken = $state<string | undefined>(undefined);
	let nextToken = $state<string | undefined>(undefined);
	let pageIndex = $derived(pageStack.length);

	let expanded = $state<string | null>(null);
	let createOpen = $state(false);
	let deleteId = $state<string | null>(null);
	let deleteName = $state<string | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	onMount(() => void fetchPage(undefined));

	async function fetchPage(token: string | undefined) {
		loading = true;
		try {
			const r = await listAppClients(poolId, { maxResults: 50, nextToken: token });
			clients = r.clients;
			nextToken = r.nextToken;
			currentToken = token;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load clients');
		} finally {
			loading = false;
		}
	}

	async function reset() {
		pageStack = [];
		currentToken = undefined;
		expanded = null;
		await fetchPage(undefined);
	}

	async function nextPage() {
		if (!nextToken) return;
		pageStack = [...pageStack, currentToken];
		await fetchPage(nextToken);
	}

	async function prevPage() {
		if (pageStack.length === 0) return;
		const newStack = [...pageStack];
		const t = newStack.pop();
		pageStack = newStack;
		await fetchPage(t);
	}

	function openDelete(id: string, name: string) {
		deleteId = id;
		deleteName = name;
		deleteOpen = true;
	}

	async function confirmDelete() {
		if (!deleteId) return;
		deleteBusy = true;
		try {
			await deleteAppClient(poolId, deleteId);
			toast.success(`Deleted ${deleteName ?? deleteId}`);
			deleteOpen = false;
			deleteId = null;
			deleteName = null;
			await fetchPage(currentToken);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div
		class="sticky top-0 z-10 flex flex-wrap items-center gap-2 border-b border-border bg-background px-6 py-3"
	>
		<Badge variant="secondary">Page {pageIndex + 1}{nextToken ? '+' : ''}</Badge>
		<div class="flex-1"></div>
		<Button
			variant="ghost"
			size="icon-sm"
			onclick={prevPage}
			disabled={pageStack.length === 0 || loading}
			title="Previous page"
		>
			<ChevronLeft class="size-4" />
		</Button>
		<Button
			variant="ghost"
			size="icon-sm"
			onclick={nextPage}
			disabled={!nextToken || loading}
			title="Next page"
		>
			<ChevronRight class="size-4" />
		</Button>
		<Button variant="ghost" size="icon-sm" onclick={reset} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
		<Button size="xs" onclick={() => (createOpen = true)}>
			<Plus class="size-3.5" /> Client
		</Button>
	</div>

	<div class="flex-1 overflow-y-auto px-6 py-4">
		{#if loading && clients.length === 0}
			<p class="text-xs text-muted-foreground">Loading clients...</p>
		{:else if clients.length === 0}
			<p class="text-xs text-muted-foreground">No app clients.</p>
		{:else}
			<ul class="space-y-1.5">
				{#each clients as c (c.clientId)}
					<li class="rounded border border-border/60">
						<div class="flex flex-wrap items-center gap-2 px-3 py-2 text-sm">
							<button
								type="button"
								class="flex min-w-0 flex-1 items-center gap-1.5 text-left"
								onclick={() => (expanded = expanded === c.clientId ? null : c.clientId)}
								aria-expanded={expanded === c.clientId}
							>
								<ChevronRight
									class="size-3.5 shrink-0 text-muted-foreground transition-transform {expanded ===
									c.clientId
										? 'rotate-90'
										: ''}"
								/>
								<div class="min-w-0">
									<div class="truncate font-medium">{c.clientName}</div>
									<div class="truncate font-mono text-xs text-muted-foreground">
										{c.clientId}
									</div>
								</div>
							</button>
							<Button
								variant="ghost"
								size="xs"
								class="text-destructive hover:text-destructive"
								onclick={() => openDelete(c.clientId, c.clientName)}
							>
								Delete
							</Button>
						</div>
						{#if expanded === c.clientId}
							<div class="border-t border-border/60 px-3 py-3">
								{#key c.clientId}
									<ClientDetail {poolId} clientId={c.clientId} />
								{/key}
							</div>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>

<CreateClientDialog
	bind:open={createOpen}
	{poolId}
	onClose={() => (createOpen = false)}
	onCreated={(id) => {
		void fetchPage(currentToken);
		expanded = id;
	}}
/>
{#if deleteId}
	<ConfirmDialog
		bind:open={deleteOpen}
		title="Delete app client"
		description={`Delete client ${deleteName ?? deleteId}? Apps using this client ID will stop working.`}
		busy={deleteBusy}
		onConfirm={confirmDelete}
		onClose={() => {
			deleteOpen = false;
			deleteId = null;
			deleteName = null;
		}}
	/>
{/if}
