<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { EmptyState, ListSkeleton } from '$lib/components/service';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { toast } from 'svelte-sonner';
	import {
		listPermissionSets,
		describePermissionSet,
		deletePermissionSet,
		type Instance,
		type PermissionSet
	} from '$lib/api/sso-admin';
	import PermissionSetDetailSheet from './permission-set-detail-sheet.svelte';

	interface Props {
		instance: Instance | null;
	}

	let { instance }: Props = $props();

	let permissionSets = $state<PermissionSet[]>([]);
	let loading = $state(false);
	let selectedArn = $state<string | null>(null);
	let sheetOpen = $state(false);
	let confirmDelete = $state<string | null>(null);

	async function load() {
		if (!instance) {
			permissionSets = [];
			return;
		}
		loading = true;
		try {
			const arns = await listPermissionSets(instance.instanceArn);
			const details = await Promise.all(
				arns.map((arn) =>
					describePermissionSet(instance.instanceArn, arn).catch(
						() => ({ permissionSetArn: arn, name: '—' }) as PermissionSet
					)
				)
			);
			permissionSets = details;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load permission sets');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		instance;
		load();
	});

	function openDetail(arn: string) {
		selectedArn = arn;
		sheetOpen = true;
	}

	async function remove(arn: string) {
		if (!instance) return;
		try {
			await deletePermissionSet(instance.instanceArn, arn);
			toast.success('Permission set deleted.');
			confirmDelete = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">Permission sets ({permissionSets.length})</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading || !instance}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	{#if !instance}
		<EmptyState
			icon={KeyRoundIcon}
			title="No instance selected"
			description="Select an Identity Center instance to view its permission sets."
		/>
	{:else}
		{#if loading && permissionSets.length === 0}
			<ListSkeleton rows={4} />
		{:else}
		<ul class="flex flex-col gap-2">
			{#if permissionSets.length === 0}
				<EmptyState
					icon={KeyRoundIcon}
					title="No permission sets"
					description="Permission sets define the access this Identity Center instance grants to accounts."
				/>
			{:else}
				{#each permissionSets as ps (ps.permissionSetArn)}
					<li class="rounded-md border border-border bg-card/40 p-3">
						<div class="flex items-start justify-between gap-3">
							<button
								type="button"
								class="min-w-0 flex-1 text-left transition-colors hover:text-primary"
								onclick={() => openDetail(ps.permissionSetArn)}
							>
								<div class="flex items-center gap-2">
									<KeyRoundIcon class="size-3.5 text-muted-foreground" />
									<span class="truncate text-xs font-medium">{ps.name || '—'}</span>
									{#if ps.sessionDuration}
										<span class="font-mono text-[10px] text-muted-foreground">
											{ps.sessionDuration}
										</span>
									{/if}
								</div>
								<p class="mt-1 truncate font-mono text-[10px] text-muted-foreground">
									{ps.permissionSetArn}
								</p>
								{#if ps.description}
									<p class="mt-1 text-[11px] text-muted-foreground">{ps.description}</p>
								{/if}
							</button>
							<Button
								size="xs"
								variant="ghost"
								class="text-destructive hover:text-destructive"
								aria-label="Delete permission set"
								onclick={() => (confirmDelete = ps.permissionSetArn)}
							>
								<Trash2Icon />
							</Button>
						</div>
					</li>
				{/each}
			{/if}
		</ul>
		{/if}
	{/if}
</div>

<!-- Confirm delete dialog -->
<Dialog
	open={confirmDelete !== null}
	onOpenChange={(o) => {
		if (!o) confirmDelete = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Delete permission set?</DialogTitle>
			<DialogDescription>
				Removes the permission set
				<span class="font-mono">{confirmDelete}</span>.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmDelete = null)}>Cancel</Button>
			<Button variant="destructive" onclick={() => confirmDelete && remove(confirmDelete)}>
				<Trash2Icon /> Delete
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>

<PermissionSetDetailSheet
	instanceArn={instance?.instanceArn ?? ''}
	permissionSetArn={selectedArn}
	open={sheetOpen}
	onOpenChange={(o) => (sheetOpen = o)}
/>
