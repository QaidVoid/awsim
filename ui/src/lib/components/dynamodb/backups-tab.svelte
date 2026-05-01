<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listBackups,
		createBackup,
		deleteBackup,
		restoreFromBackup,
		type BackupSummary,
		type TableDetail,
	} from '$lib/api/dynamodb';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle,
	} from '$lib/components/ui/dialog';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
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

	let restoreOpen = $state(false);
	let restoreFrom = $state<BackupSummary | null>(null);
	let restoreTargetName = $state('');
	let restoring = $state(false);

	$effect(() => {
		void detail.name;
		void load();
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

	async function removeBackup(b: BackupSummary) {
		if (!confirm(`Delete backup "${b.name}"?`)) return;
		try {
			await deleteBackup(b.arn);
			toast.success(`Deleted ${b.name}`);
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
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
