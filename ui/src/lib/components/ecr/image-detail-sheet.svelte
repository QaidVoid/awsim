<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle,
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { toast } from 'svelte-sonner';
	import { batchDeleteImage, type Image } from '$lib/api/ecr';

	interface Props {
		image: Image | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onDeleted?: () => void;
	}

	let { image, open = $bindable(), onOpenChange, onDeleted }: Props = $props();

	let deleting = $state(false);

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

	async function copy(value: string, label: string) {
		try {
			await navigator.clipboard.writeText(value);
			toast.success(`${label} copied.`);
		} catch {
			toast.error('Copy failed.');
		}
	}

	async function handleDelete() {
		if (!image) return;
		deleting = true;
		try {
			await batchDeleteImage(image.repositoryName, [image.imageDigest]);
			toast.success('Image deleted.');
			onOpenChange(false);
			onDeleted?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete image');
		} finally {
			deleting = false;
		}
	}
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent side="right" class="w-full max-w-2xl overflow-y-auto sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle>Image</SheetTitle>
			<SheetDescription class="truncate font-mono text-xs">
				{image?.imageDigest ?? ''}
			</SheetDescription>
		</SheetHeader>

		{#if image}
			<div class="flex flex-col gap-4 px-6 pb-6">
				<section class="rounded-md border border-border bg-card/40 p-4">
					<h3 class="mb-3 text-sm font-semibold">Manifest</h3>
					<dl class="grid grid-cols-[140px_1fr] gap-x-4 gap-y-2 text-xs">
						<dt class="text-muted-foreground">Repository</dt>
						<dd class="font-mono text-[11px]">{image.repositoryName}</dd>
						<dt class="text-muted-foreground">Tags</dt>
						<dd class="flex flex-wrap gap-1">
							{#each image.imageTags as tag (tag)}
								<Badge variant="outline" class="h-4 px-1.5 text-[10px]">{tag}</Badge>
							{:else}
								<span class="text-muted-foreground">untagged</span>
							{/each}
						</dd>
						<dt class="text-muted-foreground">Size</dt>
						<dd>{formatSize(image.imageSizeInBytes)}</dd>
						<dt class="text-muted-foreground">Pushed</dt>
						<dd>{formatDate(image.imagePushedAt)}</dd>
						<dt class="text-muted-foreground">Artifact type</dt>
						<dd class="font-mono text-[11px]">{image.artifactMediaType || '—'}</dd>
						<dt class="text-muted-foreground">Manifest type</dt>
						<dd class="font-mono text-[11px]">{image.imageManifestMediaType || '—'}</dd>
					</dl>
				</section>

				<section class="rounded-md border border-border bg-card/40 p-4">
					<div class="mb-2 flex items-center justify-between">
						<h3 class="text-sm font-semibold">Digest</h3>
						<Button
							variant="ghost"
							size="xs"
							onclick={() => copy(image.imageDigest, 'Digest')}
						>
							<CopyIcon />
							Copy
						</Button>
					</div>
					<pre
						class="overflow-auto rounded-md border border-border bg-muted/40 p-3 text-[11px] font-mono break-all whitespace-pre-wrap">{image.imageDigest}</pre>
				</section>

				<div class="flex justify-end">
					<Button variant="destructive" size="sm" onclick={handleDelete} disabled={deleting}>
						<Trash2Icon />
						{deleting ? 'Deleting…' : 'Delete image'}
					</Button>
				</div>
			</div>
		{/if}
	</SheetContent>
</Sheet>
