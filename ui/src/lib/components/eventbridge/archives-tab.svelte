<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import ArchiveIcon from '@lucide/svelte/icons/archive';
	import { toast } from 'svelte-sonner';
	import { listArchives, type Archive } from '$lib/api/eventbridge';
	import { bytesHuman } from '$lib/format';

	let archives = $state<Archive[]>([]);
	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			archives = await listArchives();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load archives');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
	});
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">Archives ({archives.length})</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	{#if archives.length === 0 && !loading}
		<EmptyState
			icon={ArchiveIcon}
			title="No archives"
			description="Archive events from a bus to enable replay. Create archives via the AWS CLI."
		/>
	{:else}
		<div class="overflow-hidden rounded-md border border-border">
			<table class="w-full text-xs">
				<thead class="border-b border-border bg-muted/30 text-left text-muted-foreground">
					<tr>
						<th class="px-3 py-2 font-medium">Name</th>
						<th class="px-3 py-2 font-medium">State</th>
						<th class="px-3 py-2 font-medium">Source bus</th>
						<th class="px-3 py-2 text-right font-medium">Retention</th>
						<th class="px-3 py-2 text-right font-medium">Events</th>
						<th class="px-3 py-2 text-right font-medium">Size</th>
					</tr>
				</thead>
				<tbody>
					{#each archives as a (a.name)}
						<tr class="border-b border-border/40 last:border-0">
							<td class="px-3 py-2 font-mono">{a.name}</td>
							<td class="px-3 py-2">
								<Badge variant="outline" class="h-4 px-1.5 text-[10px]">{a.state}</Badge>
							</td>
							<td class="truncate px-3 py-2 font-mono text-[10px] text-muted-foreground">
								{a.eventSourceArn}
							</td>
							<td class="px-3 py-2 text-right">{a.retentionDays}d</td>
							<td class="px-3 py-2 text-right">{a.eventCount}</td>
							<td class="px-3 py-2 text-right">{bytesHuman(a.sizeBytes)}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}
</div>
