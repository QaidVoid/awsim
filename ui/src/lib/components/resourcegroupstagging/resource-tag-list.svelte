<script lang="ts">
	import { onMount } from 'svelte';
	import { getResources, type TaggedResource } from '$lib/api/resourcegroupstagging';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Tag from '@lucide/svelte/icons/tag';

	let resources = $state<TaggedResource[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let filter = $state('');

	const filtered = $derived.by(() => {
		const term = filter.trim().toLowerCase();
		if (!term) return resources;
		return resources.filter((r) => {
			if (r.ResourceARN.toLowerCase().includes(term)) return true;
			return r.Tags.some(
				(t) =>
					t.Key.toLowerCase().includes(term) || t.Value.toLowerCase().includes(term)
			);
		});
	});

	async function load() {
		loading = true;
		error = null;
		try {
			resources = await getResources();
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		} finally {
			loading = false;
		}
	}

	function shortService(arn: string): string {
		const parts = arn.split(':');
		return parts.length > 2 ? parts[2] : arn;
	}

	onMount(load);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input
			type="search"
			placeholder="Filter by ARN, tag key, or value..."
			bind:value={filter}
			class="h-8 max-w-sm"
		/>
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {resources.length}</Badge>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
	</div>
	{#if error}
		<div class="border-b border-destructive/40 bg-destructive/10 px-6 py-2 text-sm">
			{error}
		</div>
	{/if}
	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
			{loading}
			columns={[
				{ key: 'service', label: 'Service', width: '140px', cell: cellService },
				{ key: 'arn', label: 'ARN', mono: true },
				{ key: 'tags', label: 'Tags', cell: cellTags }
			]}
			rowKey={(r: TaggedResource) => r.ResourceARN}
		>
			{#snippet empty()}
				<EmptyState
					icon={Tag}
					title="No tagged resources"
					description="Tag resources via TagResources to see them here. Resources tagged through service-specific APIs (e.g. PutBucketTagging) are not yet propagated to this index."
				/>
			{/snippet}
		</DataTable>
	</div>
</div>

{#snippet cellService(r: TaggedResource)}
	<Badge variant="secondary">{shortService(r.ResourceARN)}</Badge>
{/snippet}

{#snippet cellTags(r: TaggedResource)}
	{#if r.Tags.length}
		<div class="flex flex-wrap gap-1">
			{#each r.Tags as t (t.Key)}
				<Badge variant="outline" class="font-mono text-xs">
					{t.Key}={t.Value}
				</Badge>
			{/each}
		</div>
	{:else}
		<span class="text-xs text-muted-foreground">No tags</span>
	{/if}
{/snippet}
