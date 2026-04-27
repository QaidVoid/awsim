<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		describeDBSnapshots,
		deleteDBSnapshot,
		formatTimestamp,
		statusVariant,
		type DBSnapshot
	} from '$lib/api/rds';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Camera from '@lucide/svelte/icons/camera';

	let snapshots = $state<DBSnapshot[]>([]);
	let loading = $state(false);

	onMount(refresh);

	async function refresh() {
		loading = true;
		try {
			snapshots = await describeDBSnapshots();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to list snapshots');
		} finally {
			loading = false;
		}
	}

	async function remove(snap: DBSnapshot) {
		try {
			await deleteDBSnapshot(snap.identifier);
			toast.success(`Deleted ${snap.identifier}`);
			snapshots = snapshots.filter((s) => s !== snap);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete snapshot');
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div
		class="flex shrink-0 items-center justify-between border-b border-border bg-background/40 px-4 py-2"
	>
		<span class="text-xs text-muted-foreground">{snapshots.length} snapshot{snapshots.length === 1 ? '' : 's'}</span>
		<Button variant="ghost" size="icon-sm" onclick={refresh} aria-label="Refresh">
			{#if loading}
				<Loader2 class="size-3.5 animate-spin" />
			{:else}
				<RefreshCw class="size-3.5" />
			{/if}
		</Button>
	</div>

	<div class="min-h-0 flex-1 overflow-hidden">
		{#if snapshots.length === 0 && !loading}
			<div class="flex h-full items-center justify-center p-6">
				<EmptyState
					icon={Camera}
					title="No snapshots"
					description="Take an instance snapshot from the instance detail panel."
				/>
			</div>
		{:else}
			<div class="h-full overflow-auto">
				<table class="w-full text-xs">
					<thead
						class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm"
					>
						<tr>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">Snapshot</th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">Source DB</th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">Engine</th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">Type</th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">Status</th>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">Created</th>
							<th class="w-12"></th>
						</tr>
					</thead>
					<tbody>
						{#each snapshots as snap (snap.identifier)}
							<tr class="border-b border-border/40 hover:bg-muted/40">
								<td class="px-3 py-1.5 font-mono break-all">{snap.identifier}</td>
								<td class="px-3 py-1.5 font-mono text-muted-foreground">{snap.dbIdentifier}</td>
								<td class="px-3 py-1.5 font-mono text-muted-foreground">{snap.engine}</td>
								<td class="px-3 py-1.5 font-mono text-muted-foreground">{snap.snapshotType}</td>
								<td class="px-3 py-1.5">
									<Badge variant={statusVariant(snap.status)}>{snap.status}</Badge>
								</td>
								<td class="px-3 py-1.5 font-mono text-muted-foreground">
									{formatTimestamp(snap.createdAt)}
								</td>
								<td class="px-2 py-1.5">
									<Button
										variant="ghost"
										size="icon-xs"
										aria-label="Delete snapshot"
										onclick={() => remove(snap)}
									>
										<Trash2 class="size-3 text-destructive" />
									</Button>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>
</div>
