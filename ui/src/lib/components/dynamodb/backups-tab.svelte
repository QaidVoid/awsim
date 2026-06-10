<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listBackups,
		createBackup,
		deleteBackup,
		restoreFromBackup,
		describePitr,
		setPitr,
		exportTableToS3,
		listExports,
		describeExport,
		type BackupSummary,
		type ExportSummary,
		type PitrState,
		type TableDetail,
	} from '$lib/api/dynamodb';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Switch } from '$lib/components/ui/switch';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle,
	} from '$lib/components/ui/dialog';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Upload from '@lucide/svelte/icons/upload';
	import { toast } from 'svelte-sonner';

	interface Props {
		detail: TableDetail;
		onRestored?: (newTableName: string) => void;
	}

	let { detail, onRestored }: Props = $props();

	let backups = $state<BackupSummary[]>([]);
	let loading = $state(false);
	let creating = $state(false);
	let backupName = $state('');

	let pitr = $state<PitrState>({
		enabled: false,
		earliestRestorable: null,
		latestRestorable: null,
	});
	let pitrLoaded = $state(false);
	let savingPitr = $state(false);

	let exports = $state<ExportSummary[]>([]);
	let exportsLoading = $state(false);
	let exporting = $state(false);
	let exportBucket = $state('');
	let exportPrefix = $state('');

	let restoreOpen = $state(false);
	let restoreFrom = $state<BackupSummary | null>(null);
	let restoreTargetName = $state('');
	let restoring = $state(false);

	let deleteTarget = $state<BackupSummary | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	$effect(() => {
		void detail.name;
		pitrLoaded = false;
		void load();
		void loadPitr();
		void loadExports();
	});

	async function load() {
		loading = true;
		try {
			backups = await listBackups(detail.name);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load backups');
		} finally {
			loading = false;
		}
	}

	async function loadPitr() {
		try {
			pitr = await describePitr(detail.name);
		} catch {
			pitr = { enabled: false, earliestRestorable: null, latestRestorable: null };
		} finally {
			pitrLoaded = true;
		}
	}

	async function togglePitr(next: boolean) {
		savingPitr = true;
		try {
			await setPitr(detail.name, next);
			toast.success(
				next ? 'Point-in-time recovery enabled' : 'Point-in-time recovery disabled'
			);
			await loadPitr();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		} finally {
			savingPitr = false;
		}
	}

	async function loadExports() {
		if (!detail.arn) return;
		exportsLoading = true;
		try {
			exports = await listExports(detail.arn);
		} catch {
			exports = [];
		} finally {
			exportsLoading = false;
		}
	}

	async function runExport() {
		const bucket = exportBucket.trim();
		if (!bucket) {
			toast.error('Set the destination S3 bucket');
			return;
		}
		exporting = true;
		try {
			const started = await exportTableToS3(detail.arn, bucket, exportPrefix.trim() || undefined);
			// Exports settle immediately in awsim; fetch the final status
			// so the toast can report success or the S3 failure.
			const settled = await describeExport(started.arn);
			if (settled.status === 'COMPLETED') {
				toast.success(
					`Exported ${(settled.itemCount ?? 0).toLocaleString()} item${settled.itemCount === 1 ? '' : 's'} to s3://${bucket}`
				);
			} else {
				toast.error(settled.failureMessage ?? `Export ${settled.status.toLowerCase()}`);
			}
			await loadExports();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Export failed');
		} finally {
			exporting = false;
		}
	}

	function formatEpoch(secs: number | null): string {
		return secs ? new Date(secs * 1000).toLocaleString() : '—';
	}

	async function makeBackup() {
		const name = backupName.trim() || `${detail.name}-${new Date().toISOString().replace(/[:.]/g, '-')}`;
		creating = true;
		try {
			await createBackup(detail.name, name);
			toast.success(`Created backup ${name}`);
			backupName = '';
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Backup failed');
		} finally {
			creating = false;
		}
	}

	function removeBackup(b: BackupSummary) {
		deleteTarget = b;
		deleteOpen = true;
	}

	async function confirmRemoveBackup() {
		const b = deleteTarget;
		if (!b) return;
		deleteBusy = true;
		try {
			await deleteBackup(b.arn);
			toast.success(`Deleted ${b.name}`);
			deleteOpen = false;
			deleteTarget = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}

	function openRestore(b: BackupSummary) {
		restoreFrom = b;
		restoreTargetName = `${b.tableName}-restored`;
		restoreOpen = true;
	}

	async function doRestore() {
		if (!restoreFrom || !restoreTargetName.trim()) return;
		restoring = true;
		try {
			await restoreFromBackup(restoreFrom.arn, restoreTargetName.trim());
			toast.success(`Restored to ${restoreTargetName.trim()}`);
			restoreOpen = false;
			onRestored?.(restoreTargetName.trim());
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Restore failed');
		} finally {
			restoring = false;
		}
	}

	function formatBytes(n: number): string {
		if (!n) return '0 B';
		const units = ['B', 'KB', 'MB', 'GB'];
		let value = n;
		let i = 0;
		while (value >= 1024 && i < units.length - 1) {
			value /= 1024;
			i++;
		}
		const rounded = value >= 100 ? Math.round(value) : Math.round(value * 10) / 10;
		return `${rounded} ${units[i]}`;
	}
</script>

<div class="flex h-full min-h-0 flex-col gap-4 overflow-y-auto p-4">
	<section class="rounded-md border border-border p-3">
		<div class="flex items-start justify-between gap-4">
			<div class="min-w-0">
				<h3 class="text-sm font-semibold">Point-in-time recovery</h3>
				<p class="mt-0.5 text-xs text-muted-foreground">
					Tracks the restorable window for <code>RestoreTableToPointInTime</code> and is
					required before exporting to S3.
				</p>
			</div>
			<Switch
				checked={pitr.enabled}
				onCheckedChange={(v) => togglePitr(v)}
				disabled={!pitrLoaded || savingPitr}
			/>
		</div>
		{#if pitr.enabled}
			<p class="mt-2 text-xs text-muted-foreground">
				Restorable window:
				<span class="font-mono">{formatEpoch(pitr.earliestRestorable)}</span>
				to
				<span class="font-mono">{formatEpoch(pitr.latestRestorable)}</span>
			</p>
		{/if}
	</section>

	<section class="rounded-md border border-border p-3">
		<div class="mb-2 flex items-center justify-between">
			<div>
				<h3 class="text-sm font-semibold">Create backup</h3>
				<p class="mt-0.5 text-xs text-muted-foreground">
					Snapshots the schema and every item right now. Restored tables can use any new name
					and are immediately populated. Backups survive
					<code>--data-dir</code>
					restarts.
				</p>
			</div>
		</div>
		<div class="flex items-end gap-2">
			<div class="flex-1">
				<Label class="text-xs text-muted-foreground">Backup name (optional)</Label>
				<Input
					bind:value={backupName}
					placeholder={`${detail.name}-<timestamp>`}
					class="h-8 font-mono text-xs"
				/>
			</div>
			<Button size="sm" onclick={makeBackup} disabled={creating}>
				<Plus class="size-3.5" />
				<span class="ml-1">{creating ? 'Creating…' : 'Create'}</span>
			</Button>
		</div>
	</section>

	<section>
		<div class="mb-2 flex items-center justify-between">
			<h3 class="text-xs font-medium tracking-wide text-muted-foreground uppercase">
				Backups
			</h3>
			<div class="flex items-center gap-2">
				<Badge variant="outline">{backups.length}</Badge>
				<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
					<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
				</Button>
			</div>
		</div>
		{#if backups.length === 0}
			<p class="text-xs text-muted-foreground">No backups yet.</p>
		{:else}
			<table class="w-full text-xs">
				<thead>
					<tr class="border-b border-border text-left text-muted-foreground">
						<th class="py-1.5 pr-4 font-medium">Name</th>
						<th class="py-1.5 pr-4 font-medium">Created</th>
						<th class="py-1.5 pr-4 font-medium">Size</th>
						<th class="py-1.5 pr-4 font-medium">Status</th>
						<th class="py-1.5 font-medium text-right">Actions</th>
					</tr>
				</thead>
				<tbody>
					{#each backups as b (b.arn)}
						<tr class="border-b border-border/30">
							<td class="py-1.5 pr-4 font-mono">{b.name}</td>
							<td class="py-1.5 pr-4">
								{b.createdAt ? new Date(b.createdAt).toLocaleString() : '—'}
							</td>
							<td class="py-1.5 pr-4 font-mono">{formatBytes(b.sizeBytes)}</td>
							<td class="py-1.5 pr-4">
								<Badge variant={b.status === 'AVAILABLE' ? 'secondary' : 'outline'}>
									{b.status || 'UNKNOWN'}
								</Badge>
							</td>
							<td class="py-1.5 text-right">
								<Button
									variant="ghost"
									size="xs"
									onclick={() => openRestore(b)}
									disabled={b.status !== 'AVAILABLE'}
								>
									<RotateCcw class="size-3.5" />
									<span class="ml-1">Restore</span>
								</Button>
								<Button variant="ghost" size="icon-sm" onclick={() => removeBackup(b)}>
									<Trash2 class="size-3.5" />
								</Button>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</section>

	<section class="rounded-md border border-border p-3">
		<div class="mb-2">
			<h3 class="text-sm font-semibold">Export to S3</h3>
			<p class="mt-0.5 text-xs text-muted-foreground">
				Writes gzipped DynamoDB JSON plus manifests under
				<code>{'{prefix}'}/AWSDynamoDB/{'{exportId}'}/</code> in the embedded S3, the same
				layout AWS produces. Requires point-in-time recovery.
			</p>
		</div>
		<div class="flex items-end gap-2">
			<div class="flex-1">
				<Label class="text-xs text-muted-foreground">S3 bucket</Label>
				<Input
					bind:value={exportBucket}
					placeholder="my-bucket"
					class="h-8 font-mono text-xs"
				/>
			</div>
			<div class="flex-1">
				<Label class="text-xs text-muted-foreground">Prefix (optional)</Label>
				<Input
					bind:value={exportPrefix}
					placeholder="exports/"
					class="h-8 font-mono text-xs"
				/>
			</div>
			<Button
				size="sm"
				onclick={runExport}
				disabled={exporting || !pitr.enabled || !exportBucket.trim()}
				title={pitr.enabled ? undefined : 'Enable point-in-time recovery first'}
			>
				<Upload class="size-3.5" />
				<span class="ml-1">{exporting ? 'Exporting…' : 'Export'}</span>
			</Button>
		</div>

		{#if exports.length > 0 || exportsLoading}
			<div class="mt-3 flex items-center justify-between">
				<h4 class="text-xs font-medium tracking-wide text-muted-foreground uppercase">
					Exports
				</h4>
				<div class="flex items-center gap-2">
					<Badge variant="outline">{exports.length}</Badge>
					<Button
						variant="ghost"
						size="icon-sm"
						onclick={loadExports}
						disabled={exportsLoading}
						title="Refresh"
					>
						<RefreshCw class="size-3.5 {exportsLoading ? 'animate-spin' : ''}" />
					</Button>
				</div>
			</div>
			<table class="mt-1 w-full text-xs">
				<thead>
					<tr class="border-b border-border text-left text-muted-foreground">
						<th class="py-1.5 pr-4 font-medium">Destination</th>
						<th class="py-1.5 pr-4 font-medium">Started</th>
						<th class="py-1.5 pr-4 font-medium">Items</th>
						<th class="py-1.5 pr-4 font-medium">Size</th>
						<th class="py-1.5 font-medium">Status</th>
					</tr>
				</thead>
				<tbody>
					{#each exports as x (x.arn)}
						<tr class="border-b border-border/30">
							<td class="max-w-0 truncate py-1.5 pr-4 font-mono" title={x.manifestKey ?? ''}>
								s3://{x.bucket}{x.prefix ? `/${x.prefix.replace(/\/$/, '')}` : ''}
							</td>
							<td class="py-1.5 pr-4">
								{x.startTime ? new Date(x.startTime).toLocaleString() : '—'}
							</td>
							<td class="py-1.5 pr-4 font-mono">{x.itemCount?.toLocaleString() ?? '—'}</td>
							<td class="py-1.5 pr-4 font-mono">
								{x.billedSizeBytes != null ? formatBytes(x.billedSizeBytes) : '—'}
							</td>
							<td class="py-1.5" title={x.failureMessage ?? undefined}>
								<Badge variant={x.status === 'COMPLETED' ? 'secondary' : 'destructive'}>
									{x.status || 'UNKNOWN'}
								</Badge>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</section>
</div>

<Dialog bind:open={restoreOpen} onOpenChange={(v) => (restoreOpen = v)}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Restore from backup</DialogTitle>
			<DialogDescription>
				Creates a new table populated with every item captured in
				<span class="font-mono">{restoreFrom?.name ?? ''}</span>. Pick a name that doesn't
				clash with an existing table.
			</DialogDescription>
		</DialogHeader>
		<div class="space-y-1.5 py-2">
			<Label for="restore-name">New table name</Label>
			<Input id="restore-name" bind:value={restoreTargetName} class="font-mono" />
		</div>
		<DialogFooter>
			<Button variant="outline" onclick={() => (restoreOpen = false)} disabled={restoring}>
				Cancel
			</Button>
			<Button onclick={doRestore} disabled={restoring || !restoreTargetName.trim()}>
				{restoring ? 'Restoring…' : 'Restore'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete backup?"
	description={`Delete backup "${deleteTarget?.name ?? ''}".`}
	busy={deleteBusy}
	onConfirm={confirmRemoveBackup}
	onClose={() => (deleteOpen = false)}
/>
