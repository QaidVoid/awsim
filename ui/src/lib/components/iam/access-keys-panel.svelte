<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listAccessKeys,
		createAccessKey,
		updateAccessKey,
		deleteAccessKey,
		type IamAccessKey,
		type IamAccessKeyWithSecret,
	} from '$lib/api/iam';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle,
	} from '$lib/components/ui/dialog';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Power from '@lucide/svelte/icons/power';
	import Copy from '@lucide/svelte/icons/copy';
	import Key from '@lucide/svelte/icons/key';
	import { toast } from 'svelte-sonner';

	interface Props {
		userName: string;
	}

	let { userName }: Props = $props();

	let keys = $state<IamAccessKey[]>([]);
	let loading = $state(false);
	let creating = $state(false);
	let revealOpen = $state(false);
	let revealed = $state<IamAccessKeyWithSecret | null>(null);

	onMount(reload);

	$effect(() => {
		// Reload whenever the user changes — `userName` is the prop.
		void userName;
		reload();
	});

	async function reload() {
		if (!userName) return;
		loading = true;
		try {
			keys = await listAccessKeys(userName);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load access keys');
		} finally {
			loading = false;
		}
	}

	async function create() {
		creating = true;
		try {
			const k = await createAccessKey(userName);
			revealed = k;
			revealOpen = true;
			await reload();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Create failed');
		} finally {
			creating = false;
		}
	}

	async function toggle(k: IamAccessKey) {
		const next = k.status === 'Active' ? 'Inactive' : 'Active';
		try {
			await updateAccessKey(userName, k.accessKeyId, next);
			toast.success(`Set ${k.accessKeyId} to ${next}`);
			await reload();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		}
	}

	async function remove(k: IamAccessKey) {
		if (!confirm(`Delete access key ${k.accessKeyId}?`)) return;
		try {
			await deleteAccessKey(userName, k.accessKeyId);
			toast.success(`Deleted ${k.accessKeyId}`);
			await reload();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}

	function copy(value: string, label: string) {
		navigator.clipboard
			.writeText(value)
			.then(() => toast.success(`${label} copied`))
			.catch(() => toast.error('Copy failed'));
	}
</script>

<section>
	<div class="mb-2 flex items-center justify-between">
		<h3 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
			Access keys
		</h3>
		<div class="flex items-center gap-2">
			<Badge variant="outline">{keys.length} / 2</Badge>
			<Button
				variant="outline"
				size="xs"
				onclick={create}
				disabled={creating || keys.length >= 2}
				title={keys.length >= 2 ? 'AWS allows a maximum of 2 access keys per user' : 'Create new access key'}
			>
				<Plus class="size-3" />
				<span class="ml-1">Create</span>
			</Button>
		</div>
	</div>

	{#if loading}
		<p class="text-xs text-muted-foreground">Loading…</p>
	{:else if keys.length === 0}
		<p class="text-xs text-muted-foreground">No access keys.</p>
	{:else}
		<ul class="space-y-1">
			{#each keys as k (k.accessKeyId)}
				<li class="flex items-center gap-2 rounded border border-border/60 px-3 py-1.5">
					<Key class="size-3.5 shrink-0 text-muted-foreground" />
					<div class="min-w-0 flex-1">
						<div class="flex items-center gap-2">
							<span class="font-mono text-xs">{k.accessKeyId}</span>
							<Badge variant={k.status === 'Active' ? 'default' : 'secondary'} class="text-[10px]">
								{k.status}
							</Badge>
						</div>
						{#if k.createDate}
							<div class="text-[11px] text-muted-foreground">
								Created {new Date(k.createDate).toLocaleString()}
							</div>
						{/if}
					</div>
					<Button
						variant="ghost"
						size="icon-sm"
						aria-label="Toggle status"
						onclick={() => toggle(k)}
						title={k.status === 'Active' ? 'Deactivate' : 'Activate'}
					>
						<Power class="size-3.5" />
					</Button>
					<Button
						variant="ghost"
						size="icon-sm"
						aria-label="Delete access key"
						onclick={() => remove(k)}
					>
						<Trash2 class="size-3.5" />
					</Button>
				</li>
			{/each}
		</ul>
	{/if}
</section>

<Dialog bind:open={revealOpen} onOpenChange={(v) => (revealOpen = v)}>
	<DialogContent class="max-w-lg">
		<DialogHeader>
			<DialogTitle>Access key created</DialogTitle>
			<DialogDescription>
				Copy the secret now — it won't be shown again. The key is also visible in the access-keys list,
				but the secret only appears here.
			</DialogDescription>
		</DialogHeader>
		{#if revealed}
			<div class="space-y-3 py-2 text-sm">
				<div>
					<div class="text-xs uppercase text-muted-foreground">Access key ID</div>
					<div class="mt-1 flex items-center gap-2">
						<code class="flex-1 break-all rounded bg-muted px-2 py-1 font-mono text-xs">
							{revealed.accessKeyId}
						</code>
						<Button
							variant="ghost"
							size="icon-sm"
							onclick={() => copy(revealed!.accessKeyId, 'Access key ID')}
						>
							<Copy class="size-3.5" />
						</Button>
					</div>
				</div>
				<div>
					<div class="text-xs uppercase text-muted-foreground">Secret access key</div>
					<div class="mt-1 flex items-center gap-2">
						<code class="flex-1 break-all rounded bg-muted px-2 py-1 font-mono text-xs">
							{revealed.secretAccessKey}
						</code>
						<Button
							variant="ghost"
							size="icon-sm"
							onclick={() => copy(revealed!.secretAccessKey, 'Secret access key')}
						>
							<Copy class="size-3.5" />
						</Button>
					</div>
				</div>
			</div>
		{/if}
		<DialogFooter>
			<Button onclick={() => (revealOpen = false)}>Done</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
