<script lang="ts">
	import { createKeyPair, deleteKeyPair, type KeyPair } from '$lib/api/ec2';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { Textarea } from '$lib/components/ui/textarea';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter
	} from '$lib/components/ui/dialog';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import { toast } from 'svelte-sonner';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import KeyIcon from '@lucide/svelte/icons/key';
	import CopyIcon from '@lucide/svelte/icons/copy';

	interface Props {
		keys: KeyPair[];
		loading: boolean;
		onReload: () => void;
	}

	let { keys, loading, onReload }: Props = $props();

	let createOpen = $state(false);
	let formName = $state('');
	type KeyType = 'rsa' | 'ed25519';
	const KEY_TYPE_LABELS: Record<KeyType, string> = {
		rsa: 'RSA',
		ed25519: 'ED25519'
	};
	let formType = $state<KeyType>('rsa');
	let creating = $state(false);

	let materialOpen = $state(false);
	let createdMaterial = $state('');
	let createdName = $state('');

	let deleteTarget = $state<KeyPair | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	async function handleCreate(e: Event) {
		e.preventDefault();
		if (!formName.trim()) return;
		creating = true;
		try {
			const result = await createKeyPair(formName.trim(), formType);
			toast.success(`Created key pair ${result.keyName}`);
			createdMaterial = result.material;
			createdName = result.keyName;
			formName = '';
			createOpen = false;
			materialOpen = true;
			onReload();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Create failed');
		} finally {
			creating = false;
		}
	}

	function handleDelete(key: KeyPair) {
		deleteTarget = key;
		deleteOpen = true;
	}

	async function confirmDelete() {
		const key = deleteTarget;
		if (!key) return;
		deleteBusy = true;
		try {
			await deleteKeyPair(key.keyName);
			toast.success(`Deleted ${key.keyName}`);
			deleteOpen = false;
			deleteTarget = null;
			onReload();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}

	async function copyMaterial() {
		try {
			await navigator.clipboard.writeText(createdMaterial);
			toast.success('Private key copied to clipboard');
		} catch {
			toast.error('Failed to copy');
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header
		class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2"
	>
		<div class="text-xs text-muted-foreground">
			{keys.length} key pair{keys.length === 1 ? '' : 's'}
		</div>
		<div class="flex items-center gap-2">
			<Button variant="outline" size="sm" onclick={onReload} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={() => (createOpen = true)}>
				<PlusIcon />
				Create key pair
			</Button>
		</div>
	</header>

	<div class="min-h-0 flex-1">
		<DataTable
			rows={keys}
			{loading}
			rowKey={(r) => r.keyName}
			columns={[
				{ key: 'keyName', label: 'Name' },
				{ key: 'keyPairId', label: 'Key pair ID', mono: true },
				{ key: 'keyType', label: 'Type' },
				{ key: 'fingerprint', label: 'Fingerprint', mono: true },
				{ key: 'actions', label: '', align: 'right', width: '60px', cell: actionsCell }
			]}
		>
			{#snippet empty()}
				<EmptyState
					icon={KeyIcon}
					title="No key pairs"
					description="Create a key pair to SSH into your EC2 instances."
				>
					{#snippet action()}
						<Button onclick={() => (createOpen = true)}>
							<PlusIcon />
							Create key pair
						</Button>
					{/snippet}
				</EmptyState>
			{/snippet}
		</DataTable>
	</div>
</div>

{#snippet actionsCell(row: KeyPair)}
	<Button
		type="button"
		variant="ghost"
		size="icon-xs"
		onclick={() => handleDelete(row)}
		aria-label="Delete key pair"
	>
		<Trash2Icon />
	</Button>
{/snippet}

<Dialog open={createOpen} onOpenChange={(o) => (createOpen = o)}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Create key pair</DialogTitle>
			<DialogDescription>The private key will be shown once after creation.</DialogDescription>
		</DialogHeader>
		<form onsubmit={handleCreate} class="flex flex-col gap-4 py-2">
			<div class="flex flex-col gap-1.5">
				<Label for="kp-name">Key name</Label>
				<Input id="kp-name" bind:value={formName} placeholder="my-key" required />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="kp-type">Type</Label>
				<Select
					type="single"
					value={formType}
					onValueChange={(v) => (formType = v as KeyType)}
				>
					<SelectTrigger id="kp-type" class="w-full">
						{KEY_TYPE_LABELS[formType]}
					</SelectTrigger>
					<SelectContent>
						<SelectItem value="rsa" label="RSA">RSA</SelectItem>
						<SelectItem value="ed25519" label="ED25519">ED25519</SelectItem>
					</SelectContent>
				</Select>
			</div>
			<DialogFooter>
				<Button type="button" variant="ghost" onclick={() => (createOpen = false)}>Cancel</Button>
				<Button type="submit" disabled={creating || !formName.trim()}>
					<PlusIcon />
					{creating ? 'Creating...' : 'Create'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

<Dialog open={materialOpen} onOpenChange={(o) => (materialOpen = o)}>
	<DialogContent class="sm:max-w-2xl">
		<DialogHeader>
			<DialogTitle>Private key for {createdName}</DialogTitle>
			<DialogDescription>
				Save this key now — it cannot be retrieved later.
			</DialogDescription>
		</DialogHeader>
		<div class="flex flex-col gap-2 py-2">
			<Label for="kp-material">Private key (PEM)</Label>
			<Textarea
				id="kp-material"
				value={createdMaterial}
				readonly
				rows={12}
				class="font-mono text-xs"
			/>
		</div>
		<DialogFooter>
			<Button type="button" variant="outline" onclick={copyMaterial}>
				<CopyIcon />
				Copy
			</Button>
			<Button type="button" onclick={() => (materialOpen = false)}>Done</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete key pair?"
	description={`Delete key pair "${deleteTarget?.keyName ?? ''}".`}
	busy={deleteBusy}
	onConfirm={confirmDelete}
	onClose={() => (deleteOpen = false)}
/>
