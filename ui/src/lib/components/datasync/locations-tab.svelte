<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listLocations,
		deleteLocation,
		type Location
	} from '$lib/api/datasync';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import MapPin from '@lucide/svelte/icons/map-pin';
	import { toast } from 'svelte-sonner';

	interface Props {
		onSelect: (location: Location) => void;
		onCreate: () => void;
		refreshTick?: number;
	}

	let { onSelect, onCreate, refreshTick = 0 }: Props = $props();

	let locations = $state<Location[]>([]);
	let loading = $state(true);

	async function reload() {
		loading = true;
		try {
			const r = await listLocations();
			locations = r.locations;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load locations');
		} finally {
			loading = false;
		}
	}

	async function handleDelete(l: Location, e: Event) {
		e.stopPropagation();
		if (!confirm(`Delete location ${l.locationUri}?`)) return;
		try {
			await deleteLocation(l.locationArn);
			toast.success('Location deleted');
			await reload();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		}
	}

	$effect(() => {
		void refreshTick;
		reload();
	});

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{locations.length} location{locations.length === 1 ? '' : 's'}
		</div>
		<div class="flex items-center gap-2">
			<Button type="button" variant="outline" size="sm" onclick={reload} disabled={loading}>
				<RefreshCw class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button type="button" size="sm" onclick={onCreate}>
				<Plus />
				Create S3 location
			</Button>
		</div>
	</header>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if loading && locations.length === 0}
			<div class="space-y-2 p-4">
				{#each Array(4) as _, i (i)}
					<Skeleton class="h-7 w-full" />
				{/each}
			</div>
		{:else if locations.length === 0}
			<div class="p-6">
				<EmptyState
					icon={MapPin}
					title="No locations"
					description="Create an S3 location to start syncing data."
				/>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">URI</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Type</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">ARN</th>
						<th class="px-4 py-2 text-right font-medium text-muted-foreground"></th>
					</tr>
				</thead>
				<tbody>
					{#each locations as l (l.locationArn)}
						<tr
							class="cursor-pointer border-b border-border/40 hover:bg-muted/30"
							onclick={() => onSelect(l)}
						>
							<td class="px-4 py-2 font-mono text-xs">{l.locationUri}</td>
							<td class="px-4 py-2">
								<Badge variant="outline">{l.type ?? '—'}</Badge>
							</td>
							<td class="max-w-md truncate px-4 py-2 font-mono text-[11px] text-muted-foreground">
								{l.locationArn}
							</td>
							<td class="px-4 py-2 text-right">
								<Button
									type="button"
									variant="ghost"
									size="icon-xs"
									onclick={(e) => handleDelete(l, e)}
									aria-label="Delete location"
								>
									<Trash2 />
								</Button>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>
