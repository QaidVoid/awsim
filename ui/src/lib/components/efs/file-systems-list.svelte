<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import HardDriveIcon from '@lucide/svelte/icons/hard-drive';
	import { toast } from 'svelte-sonner';
	import { listFileSystems, type FileSystem } from '$lib/api/efs';

	interface Props {
		onSelect: (fs: FileSystem) => void;
		onCreate: () => void;
		refreshKey?: number;
	}

	let { onSelect, onCreate, refreshKey = 0 }: Props = $props();

	let rows = $state<FileSystem[]>([]);
	let loading = $state(false);

	$effect(() => {
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			rows = await listFileSystems();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load file systems');
		} finally {
			loading = false;
		}
	}

	function fmtBytes(n: number): string {
		if (n < 1024) return `${n} B`;
		if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
		if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
		return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
	}

	function stateColor(s: string): string {
		if (s === 'available') return 'text-green-500';
		if (s === 'deleting' || s === 'deleted') return 'text-destructive';
		return 'text-amber-500';
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			File systems
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={onCreate}>
				<PlusIcon />
				New file system
			</Button>
		</div>
	</div>

	<DataTable
		{rows}
		{loading}
		onRowClick={onSelect}
		columns={[
			{ key: 'fileSystemId', label: 'ID', mono: true },
			{ key: 'name', label: 'Name' },
			{ key: 'lifeCycleState', label: 'State', width: '110px', cell: stateCell },
			{ key: 'numberOfMountTargets', label: 'Mounts', width: '80px' },
			{ key: 'sizeInBytes', label: 'Size', width: '100px', cell: sizeCell },
			{ key: 'performanceMode', label: 'Mode', width: '140px' }
		]}
		rowKey={(r) => r.fileSystemId}
	>
		{#snippet empty()}
			<EmptyState
				icon={HardDriveIcon}
				title="No file systems"
				description="Create an EFS file system, then mount it from EC2 / Lambda / Fargate workloads."
			>
				{#snippet action()}
					<Button onclick={onCreate}>
						<PlusIcon />
						Create file system
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

{#snippet stateCell(row: FileSystem)}
	<Badge
		variant="outline"
		class={`h-5 px-2 text-[10px] ${stateColor(row.lifeCycleState)}`}
	>
		{row.lifeCycleState}
	</Badge>
{/snippet}

{#snippet sizeCell(row: FileSystem)}
	<span class="font-mono text-xs">{fmtBytes(row.sizeInBytes)}</span>
{/snippet}
