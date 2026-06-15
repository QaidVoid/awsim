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
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Camera from '@lucide/svelte/icons/camera';

	interface Props {
		instance: DBInstance;
		onDeleteInstance: (instance: DBInstance) => void;
	}

	let { instance, onDeleteInstance }: Props = $props();

	let snapshots = $state<DBSnapshot[]>([]);
	let snapshotsLoading = $state(false);
	let snapshotName = $state('');
	let creatingSnapshot = $state(false);

	$effect(() => {
		void loadSnapshots(instance.identifier);
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

<div class="border-t border-border bg-muted/20 px-4 py-4">
	<div class="grid gap-6 lg:grid-cols-2">
		<dl class="grid h-fit grid-cols-[130px_1fr] gap-y-2 text-xs">
			<dt class="text-muted-foreground">Status</dt>
			<dd>
				<Badge variant={statusVariant(instance.status)}>{instance.status || 'unknown'}</Badge>
			</dd>

			<dt class="text-muted-foreground">Engine</dt>
			<dd class="font-mono">{instance.engine} {instance.engineVersion ?? ''}</dd>

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

			{#if instance.arn}
				<dt class="text-muted-foreground">ARN</dt>
				<dd class="font-mono text-[11px] break-all">{instance.arn}</dd>
			{/if}
		</dl>

		<div class="flex flex-col gap-2">
			<div class="flex items-center gap-2">
				<span class="flex-1 text-xs font-medium text-muted-foreground">Snapshots</span>
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
					aria-label="Refresh snapshots"
					onclick={() => loadSnapshots(instance.identifier)}
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
					icon={Camera}
					title="No snapshots"
					description="Take a snapshot to create one."
				/>
			{:else}
				<table class="w-full text-xs">
					<thead>
						<tr class="border-b border-border text-left text-muted-foreground">
							<th class="py-1.5 pr-2 font-medium">Snapshot</th>
							<th class="py-1.5 pr-2 font-medium">Status</th>
							<th class="py-1.5 pr-2 font-medium">Created</th>
							<th></th>
						</tr>
					</thead>
					<tbody>
						{#each snapshots as snap (snap.identifier)}
							<tr class="border-b border-border/30">
								<td class="py-1.5 pr-2 font-mono break-all">{snap.identifier}</td>
								<td class="py-1.5 pr-2">
									<Badge variant={statusVariant(snap.status)}>{snap.status}</Badge>
								</td>
								<td class="py-1.5 pr-2 font-mono text-muted-foreground">
									{formatTimestamp(snap.createdAt)}
								</td>
								<td class="py-1.5 pl-1 text-right">
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
		</div>
	</div>

	<div class="mt-4 flex items-center justify-between border-t border-border/50 pt-3">
		<span class="text-[11px] text-muted-foreground">
			{#if instance.createdAt}Created {formatTimestamp(instance.createdAt)}{/if}
		</span>
		<Button variant="destructive" size="sm" onclick={() => onDeleteInstance(instance)}>
			<Trash2 class="size-3.5" />
			Delete instance
		</Button>
	</div>
</div>
