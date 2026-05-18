<script lang="ts">
	import { onMount } from 'svelte';
	import { listAliases, deleteAlias, type Alias } from '$lib/api/kms';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { toast } from 'svelte-sonner';
	import Tags from '@lucide/svelte/icons/tags';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import CreateAliasDialog from './create-alias-dialog.svelte';

	let aliases = $state<Alias[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let createOpen = $state(false);
	let confirmAlias = $state<Alias | null>(null);
	let confirmOpen = $state(false);
	let deleting = $state(false);

	function askDelete(a: Alias) {
		confirmAlias = a;
		confirmOpen = true;
	}

	async function doDelete() {
		const a = confirmAlias;
		if (!a) return;
		deleting = true;
		try {
			await deleteAlias(a.aliasName);
			toast.success(`Deleted ${a.aliasName}`);
			confirmOpen = false;
			confirmAlias = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete alias');
		} finally {
			deleting = false;
		}
	}

	const filtered = $derived(
		filter.trim()
			? aliases.filter((a) =>
					a.aliasName.toLowerCase().includes(filter.trim().toLowerCase())
				)
			: aliases
	);

	async function load() {
		loading = true;
		try {
			aliases = await listAliases();
		} finally {
			loading = false;
		}
	}

	onMount(load);
</script>

{#snippet aliasActions(a: Alias)}
	<Button
		variant="ghost"
		size="icon-sm"
		class="text-muted-foreground hover:text-destructive"
		aria-label="Delete {a.aliasName}"
		onclick={() => askDelete(a)}
	>
		<Trash2 class="size-3.5" />
	</Button>
{/snippet}

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input
			type="search"
			placeholder="Filter aliases..."
			bind:value={filter}
			class="h-8 max-w-xs"
		/>
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {aliases.length}</Badge>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<Plus class="size-3.5" />
			Create alias
		</Button>
	</div>
	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
			{loading}
			columns={[
				{ key: 'aliasName', label: 'Alias', width: '35%', mono: true },
				{ key: 'targetKeyId', label: 'Target key', width: '28%', mono: true },
				{ key: 'aliasArn', label: 'ARN', mono: true },
				{ key: 'actions', label: '', width: '56px', align: 'right', cell: aliasActions }
			]}
			rowKey={(r: Alias) => r.aliasArn || r.aliasName}
		>
			{#snippet empty()}
				<EmptyState
					icon={Tags}
					title="No aliases"
					description="Aliases give a friendly name to a KMS key (e.g. alias/my-app-key)."
				>
					{#snippet action()}
						<Button onclick={() => (createOpen = true)}>
							<Plus class="size-3.5" />
							Create your first alias
						</Button>
					{/snippet}
				</EmptyState>
			{/snippet}
		</DataTable>
	</div>
</div>

<CreateAliasDialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={load}
/>

<ConfirmDialog
	bind:open={confirmOpen}
	title="Delete alias?"
	description={`Delete "${confirmAlias?.aliasName ?? ''}". The target key is not affected.`}
	busy={deleting}
	onConfirm={doDelete}
	onClose={() => (confirmOpen = false)}
/>
