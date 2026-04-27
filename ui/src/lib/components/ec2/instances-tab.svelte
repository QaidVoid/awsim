<script lang="ts">
	import {
		terminateInstances,
		startInstances,
		stopInstances,
		rebootInstances,
		tagName,
		type Instance
	} from '$lib/api/ec2';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { toast } from 'svelte-sonner';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import ServerIcon from '@lucide/svelte/icons/server';
	import PowerIcon from '@lucide/svelte/icons/power';
	import PowerOffIcon from '@lucide/svelte/icons/power-off';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';

	interface Props {
		instances: Instance[];
		loading: boolean;
		onReload: () => void;
		onSelect: (instance: Instance) => void;
		onLaunch: () => void;
	}

	let { instances, loading, onReload, onSelect, onLaunch }: Props = $props();

	function stateVariant(state: string): 'default' | 'secondary' | 'destructive' | 'outline' {
		if (state === 'running') return 'default';
		if (state === 'stopped' || state === 'terminated') return 'destructive';
		if (state === 'pending' || state === 'stopping') return 'secondary';
		return 'outline';
	}

	async function action(
		fn: (ids: string[]) => Promise<void>,
		instance: Instance,
		label: string
	) {
		try {
			await fn([instance.instanceId]);
			toast.success(`${label} ${instance.instanceId}`);
			onReload();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : `${label} failed`);
		}
	}

	async function handleTerminate(instance: Instance) {
		if (!confirm(`Terminate instance ${instance.instanceId}?`)) return;
		await action(terminateInstances, instance, 'Terminating');
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header
		class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2"
	>
		<div class="text-xs text-muted-foreground">
			{instances.length} instance{instances.length === 1 ? '' : 's'}
		</div>
		<div class="flex items-center gap-2">
			<Button variant="outline" size="sm" onclick={onReload} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={onLaunch}>
				<PlusIcon />
				Launch instance
			</Button>
		</div>
	</header>

	<div class="min-h-0 flex-1">
		<DataTable
			rows={instances}
			{loading}
			rowKey={(r) => r.instanceId}
			onRowClick={onSelect}
			columns={[
				{ key: 'name', label: 'Name', cell: nameCell },
				{ key: 'instanceId', label: 'Instance ID', mono: true, cell: idCell },
				{ key: 'state', label: 'State', cell: stateCell },
				{ key: 'instanceType', label: 'Type', mono: true },
				{ key: 'privateIp', label: 'Private IP', mono: true },
				{ key: 'publicIp', label: 'Public IP', mono: true, cell: publicIpCell },
				{ key: 'az', label: 'AZ', cell: azCell },
				{ key: 'actions', label: '', align: 'right', width: '180px', cell: actionsCell }
			]}
		>
			{#snippet empty()}
				<EmptyState
					icon={ServerIcon}
					title="No instances"
					description="Launch your first EC2 instance to start running compute."
				>
					{#snippet action()}
						<Button onclick={onLaunch}>
							<PlusIcon />
							Launch instance
						</Button>
					{/snippet}
				</EmptyState>
			{/snippet}
		</DataTable>
	</div>
</div>

{#snippet nameCell(row: Instance)}
	<span class="text-sm">{tagName(row.tags) || '—'}</span>
{/snippet}
{#snippet idCell(row: Instance)}
	<span class="font-mono text-xs">{row.instanceId}</span>
{/snippet}
{#snippet stateCell(row: Instance)}
	<Badge variant={stateVariant(row.state)}>{row.state}</Badge>
{/snippet}
{#snippet publicIpCell(row: Instance)}
	<span class="font-mono text-xs">{row.publicIp || '—'}</span>
{/snippet}
{#snippet azCell(row: Instance)}
	<span class="text-xs text-muted-foreground">{row.availabilityZone || '—'}</span>
{/snippet}
{#snippet actionsCell(row: Instance)}
	<div class="flex items-center justify-end gap-0.5">
		{#if row.state === 'stopped'}
			<Button
				type="button"
				variant="ghost"
				size="icon-xs"
				onclick={(e) => {
					e.stopPropagation();
					action(startInstances, row, 'Starting');
				}}
				aria-label="Start instance"
			>
				<PowerIcon />
			</Button>
		{:else if row.state === 'running'}
			<Button
				type="button"
				variant="ghost"
				size="icon-xs"
				onclick={(e) => {
					e.stopPropagation();
					action(stopInstances, row, 'Stopping');
				}}
				aria-label="Stop instance"
			>
				<PowerOffIcon />
			</Button>
			<Button
				type="button"
				variant="ghost"
				size="icon-xs"
				onclick={(e) => {
					e.stopPropagation();
					action(rebootInstances, row, 'Rebooting');
				}}
				aria-label="Reboot instance"
			>
				<RotateCcwIcon />
			</Button>
		{/if}
		<Button
			type="button"
			variant="ghost"
			size="icon-xs"
			onclick={(e) => {
				e.stopPropagation();
				handleTerminate(row);
			}}
			aria-label="Terminate instance"
		>
			<Trash2Icon />
		</Button>
	</div>
{/snippet}
