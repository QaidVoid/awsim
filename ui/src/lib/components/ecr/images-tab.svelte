<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import LayersIcon from '@lucide/svelte/icons/layers';
	import { toast } from 'svelte-sonner';
	import { describeImages, shortDigest, type Image } from '$lib/api/ecr';

	interface Props {
		repositoryName: string;
		onSelect: (img: Image) => void;
		refreshKey?: number;
	}

	let { repositoryName, onSelect, refreshKey = 0 }: Props = $props();

	let images = $state<Image[]>([]);
	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			images = await describeImages(repositoryName);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load images');
			images = [];
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		repositoryName;
		refreshKey;
		void load();
	});

	function formatSize(bytes: number): string {
		if (!bytes) return '—';
		const units = ['B', 'KiB', 'MiB', 'GiB'];
		let v = bytes;
		let u = 0;
		while (v >= 1024 && u < units.length - 1) {
			v /= 1024;
			u += 1;
		}
		return `${v.toFixed(v < 10 ? 1 : 0)} ${units[u]}`;
	}

	function formatDate(iso: string): string {
		if (!iso) return '—';
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center justify-between gap-2 border-b border-border/40 px-4 py-2">
		<p class="text-xs text-muted-foreground">
			{loading ? 'Loading images…' : `${images.length} image${images.length === 1 ? '' : 's'}`}
		</p>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="min-h-0 flex-1">
		<DataTable
			rows={images}
			{loading}
			rowKey={(img) => img.imageDigest}
			onRowClick={onSelect}
			columns={[
				{ key: 'tags', label: 'Tags', width: '32%', cell: tagsCell },
				{ key: 'digest', label: 'Digest', width: '34%', cell: digestCell },
				{ key: 'size', label: 'Size', width: '14%', align: 'right', cell: sizeCell },
				{ key: 'pushedAt', label: 'Pushed', width: '20%', cell: pushedAtCell },
			]}
		>
			{#snippet empty()}
				<EmptyState
					icon={LayersIcon}
					title="No images pushed"
					description="Push an image with `docker push` using the repository URI to populate this view."
				/>
			{/snippet}
		</DataTable>
	</div>
</div>

{#snippet tagsCell(img: Image)}
	<div class="flex flex-wrap items-center gap-1">
		{#each img.imageTags as tag (tag)}
			<Badge variant="outline" class="h-4 px-1.5 text-[10px]">{tag}</Badge>
		{:else}
			<span class="text-xs text-muted-foreground">untagged</span>
		{/each}
	</div>
{/snippet}

{#snippet digestCell(img: Image)}
	<span class="font-mono text-[11px] text-muted-foreground">{shortDigest(img.imageDigest)}</span>
{/snippet}

{#snippet sizeCell(img: Image)}
	{formatSize(img.imageSizeInBytes)}
{/snippet}

{#snippet pushedAtCell(img: Image)}
	<span class="text-xs">{formatDate(img.imagePushedAt)}</span>
{/snippet}
