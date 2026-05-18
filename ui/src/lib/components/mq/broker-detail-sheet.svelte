<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import { toast } from 'svelte-sonner';
	import {
		describeBroker,
		deleteBroker,
		rebootBroker,
		createUser,
		deleteUser,
		type Broker,
		type BrokerSummary
	} from '$lib/api/mq';

	interface Props {
		open: boolean;
		summary: BrokerSummary | null;
		onOpenChange: (open: boolean) => void;
		onChanged?: () => void;
	}

	let { open, summary, onOpenChange, onChanged }: Props = $props();

	let broker = $state<Broker | null>(null);
	let loading = $state(false);
	let busy = $state(false);
	let newUsername = $state('');
	let deleteBrokerOpen = $state(false);
	let deleteBrokerBusy = $state(false);
	let deleteUserTarget = $state<string | null>(null);
	let deleteUserOpen = $state(false);
	let deleteUserBusy = $state(false);

	$effect(() => {
		if (open && summary) {
			void load(summary.brokerId);
		} else if (!open) {
			broker = null;
		}
	});

	async function load(id: string) {
		loading = true;
		try {
			broker = await describeBroker(id);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load broker');
		} finally {
			loading = false;
		}
	}

	async function handleReboot() {
		if (!broker) return;
		busy = true;
		try {
			await rebootBroker(broker.brokerId);
			toast.success('Broker rebooted.');
			await load(broker.brokerId);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to reboot');
		} finally {
			busy = false;
		}
	}

	function handleDelete() {
		if (!broker) return;
		deleteBrokerOpen = true;
	}

	async function confirmDeleteBroker() {
		if (!broker) return;
		deleteBrokerBusy = true;
		try {
			await deleteBroker(broker.brokerId);
			toast.success('Broker deleted.');
			deleteBrokerOpen = false;
			onChanged?.();
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		} finally {
			deleteBrokerBusy = false;
		}
	}

	async function addUser() {
		if (!broker) return;
		if (!newUsername.trim()) return toast.error('Username required.');
		busy = true;
		try {
			await createUser(broker.brokerId, newUsername.trim());
			newUsername = '';
			await load(broker.brokerId);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create user');
		} finally {
			busy = false;
		}
	}

	function removeUser(username: string) {
		if (!broker) return;
		deleteUserTarget = username;
		deleteUserOpen = true;
	}

	async function confirmRemoveUser() {
		if (!broker || !deleteUserTarget) return;
		deleteUserBusy = true;
		try {
			await deleteUser(broker.brokerId, deleteUserTarget);
			deleteUserOpen = false;
			deleteUserTarget = null;
			await load(broker.brokerId);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete user');
		} finally {
			deleteUserBusy = false;
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle>Broker details</SheetTitle>
			<SheetDescription>
				{#if summary}
					<span class="font-mono text-xs">{summary.brokerName} • {summary.brokerId}</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 pb-4">
			{#if loading}
				<p class="text-sm text-muted-foreground">Loading…</p>
			{:else if broker}
				<div class="flex flex-wrap items-center gap-2">
					<Badge variant="outline" class="h-5 px-2 text-[10px] text-green-500">
						{broker.brokerState}
					</Badge>
					<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">
						{broker.engineType} {broker.engineVersion}
					</Badge>
					<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">
						{broker.deploymentMode}
					</Badge>
				</div>

				{#if broker.brokerInstances.length > 0}
					<div class="space-y-1.5">
						<div class="text-xs font-semibold text-muted-foreground">Endpoints</div>
						{#each broker.brokerInstances as bi}
							{#each bi.endpoints as ep (ep)}
								<div class="rounded-md border border-border p-2 font-mono text-[11px] break-all">
									{ep}
								</div>
							{/each}
							{#if bi.consoleURL}
								<div class="rounded-md border border-border p-2 font-mono text-[11px] break-all">
									Console: {bi.consoleURL}
								</div>
							{/if}
						{/each}
					</div>
				{/if}

				<div class="space-y-2">
					<div class="flex items-center justify-between">
						<div class="text-xs font-semibold text-muted-foreground">
							Users ({broker.users.length})
						</div>
						<Button
							variant="ghost"
							size="xs"
							onclick={() => broker && load(broker.brokerId)}
							disabled={loading}
						>
							<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
						</Button>
					</div>
					<div class="flex items-center gap-1">
						<Input
							bind:value={newUsername}
							placeholder="username"
							class="h-7 max-w-[200px] font-mono text-xs"
						/>
						<Button size="sm" variant="outline" onclick={addUser} disabled={busy}>
							<PlusIcon class="size-3.5" />
							Add
						</Button>
					</div>
					{#if broker.users.length === 0}
						<div class="rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground">
							No users.
						</div>
					{:else}
						<div class="space-y-1.5">
							{#each broker.users as u (u.username)}
								<div class="flex items-center justify-between rounded-md border border-border p-2 text-xs">
									<span class="font-mono">{u.username}</span>
									<Button variant="ghost" size="xs" onclick={() => removeUser(u.username)}>
										<Trash2Icon class="text-destructive" />
									</Button>
								</div>
							{/each}
						</div>
					{/if}
				</div>

				<div class="flex flex-wrap items-center gap-2 border-t border-border pt-3">
					<Button size="sm" variant="outline" onclick={handleReboot} disabled={busy}>
						<RotateCcwIcon />
						Reboot
					</Button>
					<Button size="sm" variant="ghost" onclick={handleDelete} disabled={busy}>
						<Trash2Icon class="text-destructive" />
						Delete broker
					</Button>
				</div>
			{/if}
		</div>
	</SheetContent>
</Sheet>

<ConfirmDialog
	bind:open={deleteBrokerOpen}
	title="Delete broker?"
	description={`Delete broker "${broker?.brokerName ?? ''}". Users are cascaded.`}
	busy={deleteBrokerBusy}
	onConfirm={confirmDeleteBroker}
	onClose={() => (deleteBrokerOpen = false)}
/>

<ConfirmDialog
	bind:open={deleteUserOpen}
	title="Delete user?"
	description={`Delete user "${deleteUserTarget ?? ''}".`}
	busy={deleteUserBusy}
	onConfirm={confirmRemoveUser}
	onClose={() => {
		deleteUserOpen = false;
		deleteUserTarget = null;
	}}
/>
