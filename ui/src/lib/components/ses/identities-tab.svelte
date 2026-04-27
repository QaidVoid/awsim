<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
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
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import MailIcon from '@lucide/svelte/icons/mail';
	import { toast } from 'svelte-sonner';
	import {
		listIdentities,
		createIdentity,
		deleteIdentity,
		type Identity
	} from '$lib/api/ses';

	let identities = $state<Identity[]>([]);
	let loading = $state(false);
	let createOpen = $state(false);
	let newEmail = $state('');
	let creating = $state(false);
	let confirmDelete = $state<string | null>(null);

	async function load() {
		loading = true;
		try {
			identities = await listIdentities();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load identities');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
	});

	async function create() {
		if (!newEmail.trim()) {
			toast.error('Email or domain is required.');
			return;
		}
		creating = true;
		try {
			await createIdentity(newEmail.trim());
			toast.success('Identity created.');
			newEmail = '';
			createOpen = false;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create identity');
		} finally {
			creating = false;
		}
	}

	async function remove(name: string) {
		try {
			await deleteIdentity(name);
			toast.success('Identity deleted.');
			confirmDelete = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}

	function statusClass(s: string): string {
		const v = s.toUpperCase();
		if (v === 'VERIFIED' || v === 'SUCCESS') return 'text-green-500';
		if (v === 'FAILED') return 'text-destructive';
		return 'text-muted-foreground';
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">Identities ({identities.length})</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={() => (createOpen = true)}>
				<PlusIcon /> New identity
			</Button>
		</div>
	</div>

	{#snippet typeCell(i: Identity)}
		<Badge variant="outline" class="h-4 px-1.5 text-[10px]">
			{i.type.replace('_', ' ')}
		</Badge>
	{/snippet}

	{#snippet statusCell(i: Identity)}
		<Badge variant="outline" class={`h-4 px-1.5 text-[10px] ${statusClass(i.verificationStatus)}`}>
			{i.verificationStatus}
		</Badge>
	{/snippet}

	{#snippet actionsCell(i: Identity)}
		<div class="flex justify-end">
			<Button
				size="xs"
				variant="ghost"
				class="text-destructive hover:text-destructive"
				aria-label="Delete identity"
				onclick={() => (confirmDelete = i.name)}
			>
				<Trash2Icon />
			</Button>
		</div>
	{/snippet}

	<DataTable
		rows={identities}
		{loading}
		rowKey={(i) => i.name}
		columns={[
			{ key: 'name', label: 'Identity', mono: true },
			{ key: 'type', label: 'Type', width: '160px', cell: typeCell },
			{ key: 'verificationStatus', label: 'Verification', width: '140px', cell: statusCell },
			{ key: 'actions', label: '', width: '60px', align: 'right', cell: actionsCell }
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={MailIcon}
				title="No identities"
				description="Verify an email or domain before sending mail through SES."
			>
				{#snippet action()}
					<Button onclick={() => (createOpen = true)}>
						<PlusIcon /> Add your first identity
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

<Dialog open={createOpen} onOpenChange={(o) => (createOpen = o)}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New email identity</DialogTitle>
			<DialogDescription>
				Provide an email address or a domain. SES will send a verification request.
			</DialogDescription>
		</DialogHeader>
		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="ses-identity-email">Email or domain</Label>
				<Input
					id="ses-identity-email"
					bind:value={newEmail}
					placeholder="sender@example.com"
				/>
			</div>
		</div>
		<DialogFooter>
			<Button variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
			<Button onclick={create} disabled={creating || !newEmail.trim()}>
				{creating ? 'Creating…' : 'Create identity'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>

<Dialog
	open={confirmDelete !== null}
	onOpenChange={(o) => {
		if (!o) confirmDelete = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Delete identity?</DialogTitle>
			<DialogDescription>
				Removes <span class="font-mono">{confirmDelete}</span>. Sending from this identity will
				fail until reverified.
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
