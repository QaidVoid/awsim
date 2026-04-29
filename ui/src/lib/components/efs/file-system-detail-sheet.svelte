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
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import { toast } from 'svelte-sonner';
	import {
		listMountTargets,
		createMountTarget,
		deleteMountTarget,
		listAccessPoints,
		deleteAccessPoint,
		deleteFileSystem,
		type FileSystem,
		type MountTarget,
		type AccessPoint
	} from '$lib/api/efs';

	interface Props {
		open: boolean;
		fs: FileSystem | null;
		onOpenChange: (open: boolean) => void;
		onChanged?: () => void;
	}

	let { open, fs, onOpenChange, onChanged }: Props = $props();

	let mounts = $state<MountTarget[]>([]);
	let aps = $state<AccessPoint[]>([]);
	let loading = $state(false);
	let busy = $state(false);
	let newSubnet = $state('');

	$effect(() => {
		if (open && fs) {
			void load(fs.fileSystemId);
		} else if (!open) {
			mounts = [];
			aps = [];
		}
	});

	async function load(id: string) {
		loading = true;
		try {
			[mounts, aps] = await Promise.all([listMountTargets(id), listAccessPoints(id)]);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load file system detail');
		} finally {
			loading = false;
		}
	}

	async function addMount() {
		if (!fs) return;
		if (!newSubnet.trim()) return toast.error('SubnetId is required.');
		busy = true;
		try {
			await createMountTarget(fs.fileSystemId, newSubnet.trim());
			newSubnet = '';
			await load(fs.fileSystemId);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create mount target');
		} finally {
			busy = false;
		}
	}

	async function removeMount(mt: MountTarget) {
		if (!confirm(`Delete mount target ${mt.mountTargetId}?`)) return;
		try {
			await deleteMountTarget(mt.mountTargetId);
			if (fs) await load(fs.fileSystemId);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete mount target');
		}
	}

	async function removeAp(ap: AccessPoint) {
		if (!confirm(`Delete access point ${ap.accessPointId}?`)) return;
		try {
			await deleteAccessPoint(ap.accessPointId);
			if (fs) await load(fs.fileSystemId);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete access point');
		}
	}

	async function handleDelete() {
		if (!fs) return;
		if (!confirm(`Delete file system "${fs.fileSystemId}"? Mount targets must be removed first.`))
			return;
		busy = true;
		try {
			await deleteFileSystem(fs.fileSystemId);
			toast.success('File system deleted.');
			onChanged?.();
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		} finally {
			busy = false;
		}
	}

	function stateColor(s: string): string {
		if (s === 'available') return 'text-green-500';
		if (s === 'deleting' || s === 'deleted') return 'text-destructive';
		return 'text-amber-500';
	}

	function timestamp(t: number): string {
		return new Date(t * 1000).toLocaleString();
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle>File system details</SheetTitle>
			<SheetDescription>
				{#if fs}
					<span class="font-mono text-xs">{fs.fileSystemId}</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 pb-4">
			{#if loading}
				<p class="text-sm text-muted-foreground">Loading…</p>
			{:else if fs}
				<div class="flex flex-wrap items-center gap-2">
					<Badge variant="outline" class={`h-5 px-2 text-[10px] ${stateColor(fs.lifeCycleState)}`}>
						{fs.lifeCycleState}
					</Badge>
					<Badge variant="outline" class="h-5 px-2 text-[10px]">
						{fs.performanceMode}
					</Badge>
					<Badge variant="outline" class="h-5 px-2 text-[10px]">
						{fs.throughputMode}
					</Badge>
					{#if fs.encrypted}
						<Badge variant="outline" class="h-5 px-2 text-[10px] text-blue-500">encrypted</Badge>
					{/if}
				</div>

				<div class="grid grid-cols-2 gap-3 text-xs">
					<div>
						<div class="font-semibold text-muted-foreground">Created</div>
						<div>{timestamp(fs.creationTime)}</div>
					</div>
					<div>
						<div class="font-semibold text-muted-foreground">Size</div>
						<div>{fs.sizeInBytes} bytes</div>
					</div>
				</div>

				<div class="space-y-2">
					<div class="flex items-center justify-between">
						<div class="text-xs font-semibold text-muted-foreground">
							Mount targets ({mounts.length})
						</div>
						<Button
							variant="ghost"
							size="xs"
							onclick={() => fs && load(fs.fileSystemId)}
							disabled={loading}
						>
							<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
						</Button>
					</div>
					<div class="flex items-center gap-1">
						<Input
							bind:value={newSubnet}
							placeholder="subnet-xxxxxxxx"
							class="h-7 max-w-[260px] font-mono text-xs"
						/>
						<Button size="sm" variant="outline" onclick={addMount} disabled={busy}>
							<PlusIcon class="size-3.5" />
							Add
						</Button>
					</div>
					{#if mounts.length === 0}
						<div class="rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground">
							No mount targets.
						</div>
					{:else}
						<div class="space-y-1.5">
							{#each mounts as mt (mt.mountTargetId)}
								<div class="flex items-center justify-between rounded-md border border-border p-2 text-xs">
									<div class="flex flex-col gap-0.5">
										<span class="font-mono">{mt.mountTargetId}</span>
										<span class="text-muted-foreground">
											{mt.subnetId} • {mt.ipAddress} • {mt.lifeCycleState}
										</span>
									</div>
									<Button variant="ghost" size="xs" onclick={() => removeMount(mt)}>
										<Trash2Icon class="text-destructive" />
									</Button>
								</div>
							{/each}
						</div>
					{/if}
				</div>

				<div class="space-y-2">
					<div class="text-xs font-semibold text-muted-foreground">
						Access points ({aps.length})
					</div>
					{#if aps.length === 0}
						<div class="rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground">
							No access points.
						</div>
					{:else}
						<div class="space-y-1.5">
							{#each aps as ap (ap.accessPointId)}
								<div class="flex items-center justify-between rounded-md border border-border p-2 text-xs">
									<div class="flex flex-col gap-0.5">
										<span class="font-mono">{ap.accessPointId}</span>
										<span class="text-muted-foreground">
											{ap.name ?? '—'} • {ap.lifeCycleState}
										</span>
									</div>
									<Button variant="ghost" size="xs" onclick={() => removeAp(ap)}>
										<Trash2Icon class="text-destructive" />
									</Button>
								</div>
							{/each}
						</div>
					{/if}
				</div>

				<div class="flex flex-wrap items-center gap-2 border-t border-border pt-3">
					<Button size="sm" variant="ghost" onclick={handleDelete} disabled={busy}>
						<Trash2Icon class="text-destructive" />
						Delete file system
					</Button>
				</div>
			{/if}
		</div>
	</SheetContent>
</Sheet>
