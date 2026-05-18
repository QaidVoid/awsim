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
	import { Textarea } from '$lib/components/ui/textarea';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { Badge } from '$lib/components/ui/badge';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import KeyIcon from '@lucide/svelte/icons/key';
	import { toast } from 'svelte-sonner';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import {
		listUsers,
		createUser,
		deleteUser,
		importSshPublicKey,
		type ServerSummary,
		type UserSummary
	} from '$lib/api/transfer';

	interface Props {
		open: boolean;
		server: ServerSummary | null;
		onOpenChange: (open: boolean) => void;
		onChanged?: () => void;
	}

	let { open, server, onOpenChange, onChanged }: Props = $props();

	let users = $state<UserSummary[]>([]);
	let loading = $state(false);
	let busy = $state(false);
	let newUserName = $state('');
	let newRole = $state('arn:aws:iam::000000000000:role/SftpRole');
	let newHome = $state('');

	let keyForUser = $state('');
	let keyBody = $state('');

	let deleteTarget = $state<UserSummary | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	$effect(() => {
		if (open && server) {
			void load(server.serverId);
		} else if (!open) {
			users = [];
		}
	});

	async function load(id: string) {
		loading = true;
		try {
			users = await listUsers(id);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load users');
		} finally {
			loading = false;
		}
	}

	async function add() {
		if (!server) return;
		if (!newUserName.trim() || !newRole.trim())
			return toast.error('Username and Role ARN are required.');
		busy = true;
		try {
			await createUser({
				serverId: server.serverId,
				userName: newUserName.trim(),
				role: newRole.trim(),
				homeDirectory: newHome.trim() || undefined
			});
			toast.success(`Created user "${newUserName.trim()}".`);
			newUserName = '';
			newHome = '';
			await load(server.serverId);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create user');
		} finally {
			busy = false;
		}
	}

	function remove(u: UserSummary) {
		if (!server) return;
		deleteTarget = u;
		deleteOpen = true;
	}

	async function confirmRemove() {
		if (!server || !deleteTarget) return;
		deleteBusy = true;
		try {
			await deleteUser(server.serverId, deleteTarget.userName);
			toast.success('User deleted.');
			deleteOpen = false;
			deleteTarget = null;
			await load(server.serverId);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		} finally {
			deleteBusy = false;
		}
	}

	async function uploadKey() {
		if (!server) return;
		if (!keyForUser || !keyBody.trim()) return toast.error('Pick a user and paste a key body.');
		busy = true;
		try {
			const r = await importSshPublicKey(server.serverId, keyForUser, keyBody.trim());
			toast.success(`Imported key ${r.sshPublicKeyId.slice(0, 12)}…`);
			keyBody = '';
			await load(server.serverId);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to import key');
		} finally {
			busy = false;
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle>Server users</SheetTitle>
			<SheetDescription>
				{#if server}
					<span class="font-mono text-xs">{server.serverId}</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 pb-4">
			<div class="space-y-2 rounded-md border border-border p-3">
				<div class="text-xs font-semibold">Create user</div>
				<Input bind:value={newUserName} placeholder="username" class="h-8 text-xs" />
				<Input
					bind:value={newRole}
					placeholder="arn:aws:iam::...:role/SftpRole"
					class="h-8 font-mono text-xs"
				/>
				<Input
					bind:value={newHome}
					placeholder="/home/myuser (optional)"
					class="h-8 font-mono text-xs"
				/>
				<Button size="sm" onclick={add} disabled={busy}>
					<PlusIcon />
					Create user
				</Button>
			</div>

			<div class="space-y-2">
				<div class="flex items-center justify-between">
					<div class="text-xs font-semibold text-muted-foreground">Users ({users.length})</div>
					<Button
						variant="ghost"
						size="xs"
						onclick={() => server && load(server.serverId)}
						disabled={loading}
					>
						<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
					</Button>
				</div>
				{#if users.length === 0}
					<div class="rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground">
						No users.
					</div>
				{:else}
					<div class="space-y-1.5">
						{#each users as u (u.userName)}
							<div class="flex items-center justify-between rounded-md border border-border p-2 text-xs">
								<div class="flex flex-col gap-0.5">
									<span class="font-mono">{u.userName}</span>
									<span class="text-[10px] text-muted-foreground">
										{u.homeDirectory ?? '—'} • {u.homeDirectoryType}
									</span>
								</div>
								<div class="flex items-center gap-2">
									<Badge variant="outline" class="h-5 px-2 text-[10px]">
										{u.sshPublicKeyCount} key{u.sshPublicKeyCount === 1 ? '' : 's'}
									</Badge>
									<Button variant="ghost" size="xs" onclick={() => remove(u)}>
										<Trash2Icon class="text-destructive" />
									</Button>
								</div>
							</div>
						{/each}
					</div>
				{/if}
			</div>

			{#if users.length > 0}
				<div class="space-y-2 rounded-md border border-border p-3">
					<div class="text-xs font-semibold">Import SSH public key</div>
					<Select type="single" bind:value={keyForUser}>
						<SelectTrigger size="sm" class="w-full text-xs">
							{keyForUser ? keyForUser : 'Pick user...'}
						</SelectTrigger>
						<SelectContent>
							{#each users as u (u.userName)}
								<SelectItem value={u.userName} label={u.userName}
									>{u.userName}</SelectItem
								>
							{/each}
						</SelectContent>
					</Select>
					<Textarea
						bind:value={keyBody}
						rows={4}
						placeholder="ssh-rsa AAAAB3..."
						class="font-mono text-xs"
					/>
					<Button size="sm" onclick={uploadKey} disabled={busy}>
						<KeyIcon />
						Import key
					</Button>
				</div>
			{/if}
		</div>
	</SheetContent>
</Sheet>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete user?"
	description={`Delete user "${deleteTarget?.userName ?? ''}". SSH keys are cascaded.`}
	busy={deleteBusy}
	onConfirm={confirmRemove}
	onClose={() => {
		deleteOpen = false;
		deleteTarget = null;
	}}
/>
