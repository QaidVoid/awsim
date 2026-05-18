<script lang="ts">
	/**
	 * Trails tab — list of CloudTrail trails with start/stop logging,
	 * delete and create.
	 */
	import { onMount } from 'svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		describeTrails,
		getTrailStatus,
		createTrail,
		deleteTrail,
		startLogging,
		stopLogging,
		type Trail,
	} from '$lib/api/cloudtrail';
	import { EmptyState } from '$lib/components/service';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import Radar from '@lucide/svelte/icons/radar';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import { toast } from 'svelte-sonner';

	let trails = $state<Trail[]>([]);
	let loading = $state(true);
	let creating = $state(false);
	let newName = $state('');
	let newBucket = $state('');
	let busy = $state(false);
	let deleteTarget = $state<string | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	async function reload() {
		loading = true;
		try {
			const data = await describeTrails();
			const merged: Trail[] = [];
			for (const t of data.trails) {
				try {
					const s = await getTrailStatus(t.name);
					merged.push({ ...t, isLogging: s.isLogging });
				} catch {
					merged.push({ ...t, isLogging: false });
				}
			}
			trails = merged;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load trails');
		} finally {
			loading = false;
		}
	}

	async function handleCreate() {
		if (!newName.trim() || !newBucket.trim()) return;
		busy = true;
		try {
			await createTrail(newName.trim(), newBucket.trim());
			toast.success(`Created ${newName.trim()}`);
			newName = '';
			newBucket = '';
			creating = false;
			await reload();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create trail');
		} finally {
			busy = false;
		}
	}

	async function handleToggle(t: Trail) {
		try {
			if (t.isLogging) {
				await stopLogging(t.name);
				toast.success(`Stopped logging on ${t.name}`);
			} else {
				await startLogging(t.name);
				toast.success(`Started logging on ${t.name}`);
			}
			await reload();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to toggle logging');
		}
	}

	function handleDelete(name: string) {
		deleteTarget = name;
		deleteOpen = true;
	}

	async function confirmDelete() {
		const name = deleteTarget;
		if (!name) return;
		deleteBusy = true;
		try {
			await deleteTrail(name);
			toast.success(`Deleted ${name}`);
			deleteOpen = false;
			deleteTarget = null;
			await reload();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete trail');
		} finally {
			deleteBusy = false;
		}
	}

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex shrink-0 items-center gap-2 border-b border-border px-4 py-2">
		<Button size="sm" variant="ghost" class="h-7 px-2" onclick={reload} aria-label="Refresh trails">
			<RefreshCw class={`size-3.5 ${loading ? 'animate-spin' : ''}`} />
		</Button>
		<Button
			size="sm"
			variant="outline"
			class="h-7 gap-1.5 px-2 text-xs"
			onclick={() => (creating = !creating)}
		>
			<Plus class="size-3.5" /> Create trail
		</Button>
		<Badge variant="outline" class="ml-auto text-[11px]">{trails.length} total</Badge>
	</div>

	{#if creating}
		<div class="grid shrink-0 gap-3 border-b border-border bg-muted/20 p-3 sm:grid-cols-2">
			<div class="space-y-1.5">
				<Label for="ct-new-name" class="text-xs">Trail name</Label>
				<Input id="ct-new-name" bind:value={newName} class="h-8 text-xs" />
			</div>
			<div class="space-y-1.5">
				<Label for="ct-new-bucket" class="text-xs">S3 bucket</Label>
				<Input id="ct-new-bucket" bind:value={newBucket} class="h-8 text-xs" />
			</div>
			<div class="flex items-center gap-2 sm:col-span-2">
				<Button onclick={handleCreate} disabled={busy} class="h-8 text-xs">
					{busy ? 'Creating…' : 'Create'}
				</Button>
				<Button variant="ghost" class="h-8 text-xs" onclick={() => (creating = false)}>
					Cancel
				</Button>
			</div>
		</div>
	{/if}

	<div class="min-h-0 flex-1 overflow-auto">
		{#if loading && trails.length === 0}
			<div class="space-y-2 p-4">
				{#each Array(3) as _, i (i)}
					<Skeleton class="h-10 w-full" />
				{/each}
			</div>
		{:else if !loading && trails.length === 0}
			<div class="p-6">
				<EmptyState
					icon={Radar}
					title="No trails"
					description="Create one with the form above or via the AWS CLI."
				/>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead
					class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm"
				>
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Name</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">S3 bucket</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Region</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Status</th>
						<th class="px-4 py-2 text-right font-medium text-muted-foreground"></th>
					</tr>
				</thead>
				<tbody>
					{#each trails as t (t.arn || t.name)}
						<tr class="border-b border-border/40 hover:bg-muted/30">
							<td class="px-4 py-2">
								<div class="font-mono text-foreground">{t.name}</div>
								{#if t.isMultiRegionTrail}
									<Badge variant="outline" class="mt-0.5 text-[10px]">multi-region</Badge>
								{/if}
							</td>
							<td class="px-4 py-2 font-mono text-xs text-muted-foreground">
								{t.s3BucketName}
							</td>
							<td class="px-4 py-2 font-mono text-xs text-muted-foreground">
								{t.homeRegion ?? '—'}
							</td>
							<td class="px-4 py-2">
								{#if t.isLogging}
									<Badge
										variant="outline"
										class="border-emerald-500/40 bg-emerald-500/15 text-[10px] text-emerald-400"
									>
										Logging
									</Badge>
								{:else}
									<Badge variant="outline" class="text-[10px]">Stopped</Badge>
								{/if}
							</td>
							<td class="space-x-2 px-4 py-2 text-right">
								<Button
									size="sm"
									variant="ghost"
									class="h-7 px-2 text-xs"
									onclick={() => handleToggle(t)}
								>
									{t.isLogging ? 'Stop' : 'Start'}
								</Button>
								<Button
									size="sm"
									variant="ghost"
									class="h-7 px-2 text-xs text-rose-400 hover:text-rose-300"
									onclick={() => handleDelete(t.name)}
								>
									Delete
								</Button>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Delete trail?"
	description={`Delete trail "${deleteTarget ?? ''}".`}
	busy={deleteBusy}
	onConfirm={confirmDelete}
	onClose={() => (deleteOpen = false)}
/>
