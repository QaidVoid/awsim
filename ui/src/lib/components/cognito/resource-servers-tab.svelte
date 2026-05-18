<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		listResourceServers,
		createResourceServer,
		updateResourceServer,
		deleteResourceServer,
		type ResourceServer,
		type ResourceScope
	} from '$lib/api/cognito';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';

	interface Props {
		poolId: string;
	}

	let { poolId }: Props = $props();

	let servers = $state<ResourceServer[]>([]);
	let loading = $state(false);
	let expanded = $state<string | null>(null);
	let createOpen = $state(false);
	let deleteId = $state<string | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	// Editor scratch state per expanded row.
	let editScopes = $state<Record<string, ResourceScope[]>>({});
	let editName = $state<Record<string, string>>({});
	let savingId = $state<string | null>(null);
	let newScopeName = $state<Record<string, string>>({});
	let newScopeDesc = $state<Record<string, string>>({});

	onMount(load);

	async function load() {
		loading = true;
		try {
			const r = await listResourceServers(poolId);
			servers = r.servers;
			editScopes = Object.fromEntries(servers.map((s) => [s.identifier, [...s.scopes]]));
			editName = Object.fromEntries(servers.map((s) => [s.identifier, s.name]));
			newScopeName = {};
			newScopeDesc = {};
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load resource servers');
		} finally {
			loading = false;
		}
	}

	function dirty(s: ResourceServer): boolean {
		const orig = JSON.stringify(s.scopes);
		const next = JSON.stringify(editScopes[s.identifier] ?? []);
		return s.name !== (editName[s.identifier] ?? s.name) || orig !== next;
	}

	async function save(s: ResourceServer) {
		savingId = s.identifier;
		try {
			await updateResourceServer({
				poolId,
				identifier: s.identifier,
				name: editName[s.identifier] ?? s.name,
				scopes: editScopes[s.identifier] ?? []
			});
			toast.success(`Saved ${s.identifier}`);
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Save failed');
		} finally {
			savingId = null;
		}
	}

	function addScope(identifier: string) {
		const name = (newScopeName[identifier] ?? '').trim();
		const desc = (newScopeDesc[identifier] ?? '').trim();
		if (!name) return;
		const list = editScopes[identifier] ?? [];
		if (list.some((sc) => sc.name === name)) {
			toast.error(`Scope "${name}" already exists`);
			return;
		}
		editScopes[identifier] = [...list, { name, description: desc }];
		newScopeName[identifier] = '';
		newScopeDesc[identifier] = '';
	}

	function removeScope(identifier: string, name: string) {
		editScopes[identifier] = (editScopes[identifier] ?? []).filter((sc) => sc.name !== name);
	}

	function openDelete(identifier: string) {
		deleteId = identifier;
		deleteOpen = true;
	}

	async function confirmDelete() {
		if (!deleteId) return;
		deleteBusy = true;
		try {
			await deleteResourceServer(poolId, deleteId);
			toast.success(`Deleted ${deleteId}`);
			deleteOpen = false;
			deleteId = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}

	// Create dialog state
	let cName = $state('');
	let cIdentifier = $state('');
	let cScopes = $state<ResourceScope[]>([]);
	let cNewName = $state('');
	let cNewDesc = $state('');
	let cSaving = $state(false);
	let cError = $state<string | null>(null);

	$effect(() => {
		if (!createOpen) {
			cName = '';
			cIdentifier = '';
			cScopes = [];
			cNewName = '';
			cNewDesc = '';
			cSaving = false;
			cError = null;
		}
	});

	function cAddScope() {
		const n = cNewName.trim();
		if (!n) return;
		if (cScopes.some((sc) => sc.name === n)) return;
		cScopes = [...cScopes, { name: n, description: cNewDesc.trim() }];
		cNewName = '';
		cNewDesc = '';
	}

	async function cSubmit() {
		if (!cIdentifier.trim()) {
			cError = 'Identifier is required';
			return;
		}
		if (!cName.trim()) {
			cError = 'Friendly name is required';
			return;
		}
		cSaving = true;
		cError = null;
		try {
			await createResourceServer({
				poolId,
				identifier: cIdentifier.trim(),
				name: cName.trim(),
				scopes: cScopes
			});
			toast.success(`Created ${cIdentifier.trim()}`);
			createOpen = false;
			await load();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Create failed';
			cError = msg;
			toast.error(msg);
		} finally {
			cSaving = false;
		}
	}
</script>

<div class="space-y-3">
	<div class="flex items-center gap-2">
		<p class="text-xs text-muted-foreground">
			Custom OAuth scopes — emitted in <code>scope</code> claim of access tokens for
			consenting apps.
		</p>
		<div class="flex-1"></div>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
		<Button size="xs" onclick={() => (createOpen = true)}>
			<Plus class="size-3.5" /> Resource server
		</Button>
	</div>

	{#if loading && servers.length === 0}
		<p class="text-xs text-muted-foreground">Loading...</p>
	{:else if servers.length === 0}
		<p class="text-xs text-muted-foreground">No resource servers configured.</p>
	{:else}
		<ul class="space-y-1.5">
			{#each servers as s (s.identifier)}
				<li class="rounded border border-border/60">
					<div class="flex flex-wrap items-center gap-2 px-3 py-2 text-sm">
						<button
							type="button"
							class="flex min-w-0 flex-1 items-center gap-1.5 text-left"
							onclick={() => (expanded = expanded === s.identifier ? null : s.identifier)}
							aria-expanded={expanded === s.identifier}
						>
							<ChevronRight
								class="size-3.5 shrink-0 text-muted-foreground transition-transform {expanded ===
								s.identifier
									? 'rotate-90'
									: ''}"
							/>
							<div class="min-w-0">
								<div class="truncate font-medium">{s.name}</div>
								<code class="truncate font-mono text-xs text-muted-foreground">
									{s.identifier}
								</code>
							</div>
							<Badge variant="outline" class="font-mono text-[10px]">
								{s.scopes.length} scope{s.scopes.length === 1 ? '' : 's'}
							</Badge>
						</button>
						<Button
							variant="ghost"
							size="xs"
							class="text-destructive hover:text-destructive"
							onclick={() => openDelete(s.identifier)}
						>
							Delete
						</Button>
					</div>
					{#if expanded === s.identifier}
						<div class="space-y-3 border-t border-border/60 px-3 py-3 text-sm">
							<div class="space-y-1.5">
								<Label class="text-xs">Friendly name</Label>
								<Input
									bind:value={editName[s.identifier]}
									class="h-7 text-xs"
								/>
							</div>
							<div class="space-y-1.5">
								<Label class="text-xs">Scopes</Label>
								{#if (editScopes[s.identifier] ?? []).length === 0}
									<p class="text-xs text-muted-foreground">No scopes.</p>
								{:else}
									<ul class="space-y-1">
										{#each editScopes[s.identifier] as sc (sc.name)}
											<li class="grid grid-cols-[10rem_minmax(0,1fr)_auto] items-center gap-2">
												<Badge variant="outline" class="font-mono text-[10px]">
													{s.identifier}/{sc.name}
												</Badge>
												<span class="truncate text-xs">{sc.description}</span>
												<Button
													variant="ghost"
													size="icon-sm"
													onclick={() => removeScope(s.identifier, sc.name)}
													class="text-destructive hover:text-destructive"
												>
													<X class="size-3" />
												</Button>
											</li>
										{/each}
									</ul>
								{/if}
								<div class="grid grid-cols-[10rem_minmax(0,1fr)_auto] items-center gap-2 pt-1">
									<Input
										placeholder="scope name"
										bind:value={newScopeName[s.identifier]}
										class="h-7 font-mono text-xs"
									/>
									<Input
										placeholder="description"
										bind:value={newScopeDesc[s.identifier]}
										class="h-7 min-w-0 text-xs"
									/>
									<Button
										size="xs"
										onclick={() => addScope(s.identifier)}
										disabled={!(newScopeName[s.identifier] ?? '').trim()}
									>
										<Plus class="size-3" />
									</Button>
								</div>
							</div>
							<div class="flex justify-end">
								<Button
									size="sm"
									onclick={() => save(s)}
									disabled={savingId === s.identifier || !dirty(s)}
								>
									{#if savingId === s.identifier}<Loader2 class="size-3.5 animate-spin" />{/if}
									Save
								</Button>
							</div>
						</div>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</div>

<Dialog bind:open={createOpen} onOpenChange={(v: boolean) => !v && (createOpen = false)}>
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle>Create resource server</DialogTitle>
			<DialogDescription>
				A logical API resource that defines scopes clients can request.
			</DialogDescription>
		</DialogHeader>
		<form
			class="flex flex-col gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void cSubmit();
			}}
		>
			<div class="space-y-1.5">
				<Label for="rs-name">Friendly name</Label>
				<Input id="rs-name" bind:value={cName} placeholder="my-api" />
			</div>
			<div class="space-y-1.5">
				<Label for="rs-id">Identifier (used as scope prefix)</Label>
				<Input
					id="rs-id"
					bind:value={cIdentifier}
					placeholder="https://api.example.com"
					class="font-mono text-xs"
				/>
			</div>
			<div class="space-y-1.5">
				<Label class="text-xs">Scopes</Label>
				{#if cScopes.length > 0}
					<ul class="space-y-1">
						{#each cScopes as sc, i (sc.name)}
							<li class="flex items-center gap-2 text-sm">
								<Badge variant="outline" class="font-mono text-[10px]">{sc.name}</Badge>
								<span class="flex-1 truncate text-xs text-muted-foreground">{sc.description}</span>
								<Button
									variant="ghost"
									size="icon-sm"
									onclick={() => (cScopes = cScopes.filter((_, j) => j !== i))}
									class="text-destructive hover:text-destructive"
								>
									<X class="size-3" />
								</Button>
							</li>
						{/each}
					</ul>
				{/if}
				<div class="grid grid-cols-[10rem_minmax(0,1fr)_auto] items-center gap-2">
					<Input
						placeholder="scope name"
						bind:value={cNewName}
						class="h-7 font-mono text-xs"
					/>
					<Input
						placeholder="description"
						bind:value={cNewDesc}
						class="h-7 min-w-0 text-xs"
					/>
					<Button size="xs" onclick={cAddScope} disabled={!cNewName.trim()}>
						<Plus class="size-3" />
					</Button>
				</div>
			</div>
			{#if cError}
				<p class="text-xs text-destructive">{cError}</p>
			{/if}
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					onclick={() => (createOpen = false)}
					disabled={cSaving}
				>
					Cancel
				</Button>
				<Button type="submit" disabled={cSaving || !cIdentifier.trim() || !cName.trim()}>
					{#if cSaving}<Loader2 class="size-3.5 animate-spin" />{/if}
					Create
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

{#if deleteId}
	<ConfirmDialog
		bind:open={deleteOpen}
		title="Delete resource server"
		description={`Delete "${deleteId}"? Apps requesting its scopes will fail.`}
		busy={deleteBusy}
		onConfirm={confirmDelete}
		onClose={() => {
			deleteOpen = false;
			deleteId = null;
		}}
	/>
{/if}
