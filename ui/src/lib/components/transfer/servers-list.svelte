<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import PowerIcon from '@lucide/svelte/icons/power';
	import PowerOffIcon from '@lucide/svelte/icons/power-off';
	import UploadIcon from '@lucide/svelte/icons/upload';
	import { toast } from 'svelte-sonner';
	import {
		listServers,
		createServer,
		deleteServer,
		startServer,
		stopServer,
		type ServerSummary
	} from '$lib/api/transfer';

	interface Props {
		onSelect: (s: ServerSummary) => void;
		refreshKey?: number;
	}

	let { onSelect, refreshKey = 0 }: Props = $props();

	let rows = $state<ServerSummary[]>([]);
	let loading = $state(false);
	let creating = $state(false);

	$effect(() => {
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			rows = await listServers();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load servers');
		} finally {
			loading = false;
		}
	}

	async function create() {
		creating = true;
		try {
			const s = await createServer(['SFTP']);
			toast.success(`Created server ${s.serverId}.`);
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create server');
		} finally {
			creating = false;
		}
	}

	async function toggle(s: ServerSummary, ev: MouseEvent) {
		ev.stopPropagation();
		try {
			if (s.state === 'ONLINE') {
				await stopServer(s.serverId);
				toast.success('Server stopped.');
			} else {
				await startServer(s.serverId);
				toast.success('Server started.');
			}
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to toggle server');
		}
	}

	async function remove(s: ServerSummary, ev: MouseEvent) {
		ev.stopPropagation();
		if (!confirm(`Delete server ${s.serverId}? Users + SSH keys are cascaded.`)) return;
		try {
			await deleteServer(s.serverId);
			toast.success('Server deleted.');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}

	function stateColor(s: string): string {
		if (s === 'ONLINE') return 'text-green-500';
		if (s === 'OFFLINE') return 'text-muted-foreground';
		return 'text-amber-500';
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Servers
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={create} disabled={creating}>
				<PlusIcon />
				{creating ? 'Creating…' : 'Create SFTP server'}
			</Button>
		</div>
	</div>

	<DataTable
		{rows}
		{loading}
		onRowClick={onSelect}
		columns={[
			{ key: 'serverId', label: 'ID', mono: true },
			{ key: 'state', label: 'State', width: '110px', cell: stateCell },
			{ key: 'identityProviderType', label: 'Identity', width: '180px', mono: true },
			{ key: 'endpointType', label: 'Endpoint', width: '110px', mono: true },
			{ key: 'userCount', label: 'Users', width: '80px' },
			{ key: '__actions', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.serverId}
	>
		{#snippet empty()}
			<EmptyState
				icon={UploadIcon}
				title="No transfer servers"
				description="Create a Transfer Family server (SFTP / FTPS / FTP). AWSim never starts an actual listener — state flips to ONLINE on Create."
			>
				{#snippet action()}
					<Button onclick={create} disabled={creating}>
						<PlusIcon />
						Create SFTP server
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

{#snippet stateCell(row: ServerSummary)}
	<Badge variant="outline" class={`h-5 px-2 text-[10px] ${stateColor(row.state)}`}>
		{row.state}
	</Badge>
{/snippet}

{#snippet actionsCell(row: ServerSummary)}
	<div class="flex items-center gap-1">
		<Button variant="ghost" size="xs" onclick={(e) => toggle(row, e)}>
			{#if row.state === 'ONLINE'}<PowerOffIcon />{:else}<PowerIcon />{/if}
		</Button>
		<Button variant="ghost" size="xs" onclick={(e) => remove(row, e)}>
			<Trash2Icon class="text-destructive" />
		</Button>
	</div>
{/snippet}
