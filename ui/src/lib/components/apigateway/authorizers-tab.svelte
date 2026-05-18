<script lang="ts">
	import {
		getAuthorizers,
		createAuthorizer,
		deleteAuthorizer,
		type Authorizer,
	} from '$lib/api/apigateway';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle,
	} from '$lib/components/ui/dialog';
	import { toast } from 'svelte-sonner';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';

	interface Props {
		restApiId: string;
	}

	let { restApiId }: Props = $props();

	let authorizers = $state<Authorizer[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);

	let createOpen = $state(false);
	let creating = $state(false);
	let newName = $state('');
	let newType = $state<'TOKEN' | 'REQUEST' | 'COGNITO_USER_POOLS'>('TOKEN');
	let newAuthType = $state('custom');
	let newAuthorizerUri = $state('');
	let newIdentitySource = $state('method.request.header.Authorization');

	let deleteTarget = $state<Authorizer | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	const TYPES = ['TOKEN', 'REQUEST', 'COGNITO_USER_POOLS'] as const;

	async function load() {
		loading = true;
		error = null;
		try {
			authorizers = await getAuthorizers(restApiId);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load authorizers';
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		if (restApiId) load();
	});

	function openCreate() {
		newName = '';
		newType = 'TOKEN';
		newAuthType = 'custom';
		newAuthorizerUri = '';
		newIdentitySource = 'method.request.header.Authorization';
		createOpen = true;
	}

	async function submitCreate(e: Event) {
		e.preventDefault();
		if (!newName.trim()) return;
		creating = true;
		try {
			await createAuthorizer(restApiId, {
				name: newName.trim(),
				type: newType,
				authType: newAuthType.trim() || undefined,
				authorizerUri: newAuthorizerUri.trim() || undefined,
				identitySource: newIdentitySource.trim() || undefined,
			});
			toast.success(`Authorizer ${newName.trim()} created`);
			createOpen = false;
			await load();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Create failed');
		} finally {
			creating = false;
		}
	}

	function remove(a: Authorizer) {
		deleteTarget = a;
		deleteOpen = true;
	}

	async function confirmRemove() {
		const a = deleteTarget;
		if (!a) return;
		deleteBusy = true;
		try {
			await deleteAuthorizer(restApiId, a.id);
			toast.success(`Authorizer ${a.name || a.id} deleted`);
			deleteOpen = false;
			deleteTarget = null;
			await load();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex shrink-0 items-center justify-end border-b border-border bg-background/40 px-4 py-2">
		<Button size="sm" onclick={openCreate} class="h-7 gap-1 px-2.5">
			<Plus class="size-3.5" />
			<span class="text-xs">New authorizer</span>
		</Button>
	</div>

	<div class="min-h-0 flex-1 overflow-y-auto p-4">
		{#if loading}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if error}
			<div class="text-sm text-destructive">{error}</div>
		{:else if authorizers.length === 0}
			<div class="text-sm text-muted-foreground">No authorizers configured.</div>
		{:else}
			<ul class="flex flex-col gap-2">
				{#each authorizers as a (a.id)}
					<li class="rounded-md border border-border bg-card/40 p-3">
						<div class="mb-2 flex items-center gap-2">
							<span class="text-sm font-medium">{a.name || a.id}</span>
							<span class="rounded bg-muted px-1.5 py-0.5 font-mono text-[10px]">
								{a.type}
							</span>
							<Button
								size="sm"
								variant="ghost"
								class="ml-auto h-6 gap-1 px-1.5 text-destructive"
								onclick={() => remove(a)}
								aria-label="Delete authorizer"
							>
								<Trash2 class="size-3.5" />
							</Button>
						</div>
						<div class="grid grid-cols-[120px_1fr] gap-x-2 gap-y-0.5 text-xs">
							<span class="text-muted-foreground">Auth type</span>
							<span class="font-mono">{a.authType || '—'}</span>
							<span class="text-muted-foreground">Identity source</span>
							<span class="truncate font-mono">{a.identitySource || '—'}</span>
							{#if a.authorizerUri}
								<span class="text-muted-foreground">Authorizer URI</span>
								<span class="truncate font-mono">{a.authorizerUri}</span>
							{/if}
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>

<Dialog bind:open={createOpen}>
	<DialogContent>
		<DialogHeader>
			<DialogTitle>Create authorizer</DialogTitle>
			<DialogDescription>
				Authorizers are stored but not enforced at request time — they exist
				so SDK code that creates them round-trips against AWSim.
			</DialogDescription>
		</DialogHeader>
		<form onsubmit={submitCreate} class="space-y-3">
			<div class="space-y-1">
				<Label for="auth-name">Name</Label>
				<Input id="auth-name" bind:value={newName} required />
			</div>
			<div class="grid grid-cols-2 gap-3">
				<div class="space-y-1">
					<Label for="auth-type">Type</Label>
					<select
						id="auth-type"
						bind:value={newType}
						class="h-9 w-full rounded-md border border-border bg-background px-2 text-sm"
					>
						{#each TYPES as t (t)}
							<option value={t}>{t}</option>
						{/each}
					</select>
				</div>
				<div class="space-y-1">
					<Label for="auth-authtype">authType</Label>
					<Input id="auth-authtype" bind:value={newAuthType} class="font-mono text-xs" />
				</div>
			</div>
			<div class="space-y-1">
				<Label for="auth-uri">Authorizer URI (optional)</Label>
				<Input
					id="auth-uri"
					bind:value={newAuthorizerUri}
					placeholder="arn:aws:lambda:..."
					class="font-mono text-xs"
				/>
			</div>
			<div class="space-y-1">
				<Label for="auth-source">Identity source</Label>
				<Input id="auth-source" bind:value={newIdentitySource} class="font-mono text-xs" />
			</div>
			<DialogFooter>
				<Button type="submit" disabled={creating || !newName.trim()}>
					{creating ? 'Creating...' : 'Create'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete authorizer?"
	description={`Permanently delete authorizer "${deleteTarget?.name || deleteTarget?.id || ''}".`}
	busy={deleteBusy}
	onConfirm={confirmRemove}
	onClose={() => (deleteOpen = false)}
/>
