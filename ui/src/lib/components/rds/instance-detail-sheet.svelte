<script lang="ts">
	import { toast } from 'svelte-sonner';
	import {
		describeDBSnapshots,
		createDBSnapshot,
		deleteDBSnapshot,
		formatTimestamp,
		statusVariant,
		type DBInstance,
		type DBSnapshot
	} from '$lib/api/rds';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import { EmptyState } from '$lib/components/service';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Plus from '@lucide/svelte/icons/plus';
	import Camera from '@lucide/svelte/icons/camera';

	interface Props {
		open: boolean;
		instance: DBInstance | null;
		onClose: () => void;
		onDeleteInstance: (instance: DBInstance) => void;
	}

	let { open = $bindable(false), instance, onClose, onDeleteInstance }: Props = $props();

	let snapshots = $state<DBSnapshot[]>([]);
	let snapshotsLoading = $state(false);
	let snapshotName = $state('');
	let creatingSnapshot = $state(false);

	let activeTab = $state<'overview' | 'snapshots'>('overview');

	$effect(() => {
		if (open && instance) {
			activeTab = 'overview';
			snapshotName = '';
			void loadSnapshots(instance.identifier);
		}
	});

	async function loadSnapshots(id: string) {
		snapshotsLoading = true;
		try {
			snapshots = await describeDBSnapshots(id);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to list snapshots');
		} finally {
			snapshotsLoading = false;
		}
	}

	async function takeSnapshot() {
		if (!instance) return;
		const id =
			snapshotName.trim() ||
			`${instance.identifier}-${new Date().toISOString().replace(/[^0-9]/g, '').slice(0, 14)}`;
		creatingSnapshot = true;
		try {
			await createDBSnapshot(instance.identifier, id);
			toast.success(`Snapshot ${id} requested`);
			snapshotName = '';
			await loadSnapshots(instance.identifier);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create snapshot');
		} finally {
			creatingSnapshot = false;
		}
	}

	async function removeSnapshot(snap: DBSnapshot) {
		try {
			await deleteDBSnapshot(snap.identifier);
			toast.success(`Deleted ${snap.identifier}`);
			snapshots = snapshots.filter((s) => s !== snap);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete snapshot');
		}
	}
</script>

<Sheet bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<SheetContent class="flex w-full flex-col gap-0 p-0 sm:max-w-xl">
		<SheetHeader class="border-b border-border p-4">
			<SheetTitle class="font-mono text-sm">{instance?.identifier ?? ''}</SheetTitle>
			<SheetDescription>
				{instance?.engine}
				{instance?.engineVersion ?? ''}
			</SheetDescription>
		</SheetHeader>

		<Tabs bind:value={activeTab} class="flex min-h-0 flex-1 flex-col gap-0">
			<TabsList class="mx-4 mt-2 self-start">
				<TabsTrigger value="overview">Overview</TabsTrigger>
				<TabsTrigger value="snapshots">Snapshots</TabsTrigger>
			</TabsList>

			<div class="min-h-0 flex-1 overflow-y-auto">
				<TabsContent value="overview" class="m-0 h-full p-4">
					{#if !instance}
						<p class="text-sm text-muted-foreground">No instance selected.</p>
					{:else}
						<dl class="grid grid-cols-[140px_1fr] gap-y-2 text-xs">
							<dt class="text-muted-foreground">Status</dt>
							<dd>
								<Badge variant={statusVariant(instance.status)}>
									{instance.status || 'unknown'}
								</Badge>
							</dd>

							<dt class="text-muted-foreground">Engine</dt>
							<dd class="font-mono">
								{instance.engine}
								{instance.engineVersion ?? ''}
							</dd>

							<dt class="text-muted-foreground">Class</dt>
							<dd class="font-mono">{instance.instanceClass}</dd>

							<dt class="text-muted-foreground">Endpoint</dt>
							<dd class="font-mono break-all">
								{instance.endpoint || '—'}{instance.port ? `:${instance.port}` : ''}
							</dd>

							<dt class="text-muted-foreground">Storage</dt>
							<dd class="font-mono">
								{instance.allocatedStorage} GiB
								{instance.storageType ? `(${instance.storageType})` : ''}
							</dd>

							<dt class="text-muted-foreground">Master user</dt>
							<dd class="font-mono">{instance.masterUsername || '—'}</dd>

							<dt class="text-muted-foreground">Multi-AZ</dt>
							<dd>{instance.multiAZ ? 'Yes' : 'No'}</dd>

							<dt class="text-muted-foreground">Public</dt>
							<dd>{instance.publiclyAccessible ? 'Yes' : 'No'}</dd>

							{#if instance.createdAt}
								<dt class="text-muted-foreground">Created</dt>
								<dd class="font-mono">{formatTimestamp(instance.createdAt)}</dd>
							{/if}

							{#if instance.arn}
								<dt class="text-muted-foreground">ARN</dt>
								<dd class="font-mono text-[11px] break-all">{instance.arn}</dd>
							{/if}
						</dl>
					{/if}
				</TabsContent>

				<TabsContent value="snapshots" class="m-0 h-full p-4">
					<div class="mb-3 flex items-center gap-2">
						<Input
							bind:value={snapshotName}
							placeholder="snapshot name (auto if empty)"
							class="h-8 flex-1 text-xs"
						/>
						<Button size="sm" onclick={takeSnapshot} disabled={creatingSnapshot}>
							{#if creatingSnapshot}
								<Loader2 class="size-3.5 animate-spin" />
							{:else}
								<Camera class="size-3.5" />
							{/if}
							Snapshot
						</Button>
						<Button
							variant="ghost"
							size="icon-sm"
							aria-label="Refresh"
							onclick={() => instance && loadSnapshots(instance.identifier)}
						>
							{#if snapshotsLoading}
								<Loader2 class="size-3.5 animate-spin" />
							{:else}
								<RefreshCw class="size-3.5" />
							{/if}
						</Button>
					</div>

					{#if snapshots.length === 0 && !snapshotsLoading}
						<EmptyState
							icon={Plus}
							title="No snapshots"
							description="Take a snapshot to create one."
						/>
					{:else}
						<table class="w-full text-xs">
							<thead>
								<tr class="border-b border-border text-left text-muted-foreground">
									<th class="py-1.5 pr-2 font-medium">Snapshot</th>
									<th class="py-1.5 pr-2 font-medium">Status</th>
									<th class="py-1.5 pr-2 font-medium">Type</th>
									<th class="py-1.5 pr-2 font-medium">Created</th>
									<th></th>
								</tr>
							</thead>
							<tbody>
								{#each snapshots as snap (snap.identifier)}
									<tr class="border-b border-border/30">
										<td class="py-1.5 pr-2 font-mono break-all">{snap.identifier}</td>
										<td class="py-1.5 pr-2">
											<Badge variant={statusVariant(snap.status)}>
												{snap.status}
											</Badge>
										</td>
										<td class="py-1.5 pr-2 font-mono">{snap.snapshotType}</td>
										<td class="py-1.5 pr-2 font-mono text-muted-foreground">
											{formatTimestamp(snap.createdAt)}
										</td>
										<td class="py-1.5 pl-1">
											<Button
												variant="ghost"
												size="icon-xs"
												aria-label="Delete snapshot"
												onclick={() => removeSnapshot(snap)}
											>
												<Trash2 class="size-3 text-destructive" />
											</Button>
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					{/if}
				</TabsContent>
			</div>
		</Tabs>

		<footer class="flex shrink-0 items-center justify-end gap-2 border-t border-border p-4">
			<Button
				variant="destructive"
				size="sm"
				onclick={() => instance && onDeleteInstance(instance)}
				disabled={!instance}
			>
				<Trash2 class="size-3.5" />
				Delete instance
			</Button>
		</footer>
	</SheetContent>
</Sheet>
