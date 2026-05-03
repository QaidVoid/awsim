<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import {
		formatBytes,
		formatTimestamp,
		headObject,
		getObjectText,
		getObjectBlob,
		objectUrl,
		type ObjectMetadata,
		type S3Object
	} from '$lib/api/s3';
	import { toast } from 'svelte-sonner';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Download from '@lucide/svelte/icons/download';
	import Eye from '@lucide/svelte/icons/eye';
	import Trash2 from '@lucide/svelte/icons/trash-2';

	interface Props {
		open: boolean;
		bucket: string | null;
		object: S3Object | null;
		onClose: () => void;
		onDelete: (obj: S3Object) => void;
	}

	let { open = $bindable(false), bucket, object, onClose, onDelete }: Props = $props();

	let metadata = $state<ObjectMetadata | null>(null);
	let metaLoading = $state(false);
	let preview = $state<string | null>(null);
	let previewLoading = $state(false);
	let previewError = $state<string | null>(null);
	let imageBlobUrl = $state<string | null>(null);

	$effect(() => {
		if (open && bucket && object) {
			metadata = null;
			preview = null;
			previewError = null;
			if (imageBlobUrl) URL.revokeObjectURL(imageBlobUrl);
			imageBlobUrl = null;
			void loadMetadata(bucket, object.key);
		}
	});

	async function loadMetadata(b: string, key: string) {
		metaLoading = true;
		try {
			metadata = await headObject(b, key);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load metadata');
		} finally {
			metaLoading = false;
		}
	}

	async function loadPreview() {
		if (!bucket || !object) return;
		previewLoading = true;
		previewError = null;
		try {
			preview = await getObjectText(bucket, object.key);
		} catch (e) {
			previewError = e instanceof Error ? e.message : 'Failed to load preview';
		} finally {
			previewLoading = false;
		}
	}

	function downloadUrl(): string {
		if (!bucket || !object) return '#';
		return objectUrl(bucket, object.key);
	}

	async function loadImagePreview() {
		if (!bucket || !object) return;
		try {
			const blob = await getObjectBlob(bucket, object.key);
			if (imageBlobUrl) URL.revokeObjectURL(imageBlobUrl);
			imageBlobUrl = URL.createObjectURL(blob);
		} catch {
			imageBlobUrl = null;
		}
	}

	async function downloadFile() {
		if (!bucket || !object) return;
		try {
			const blob = await getObjectBlob(bucket, object.key);
			const url = URL.createObjectURL(blob);
			const a = document.createElement('a');
			a.href = url;
			a.download = object.key.split('/').pop() ?? object.key;
			document.body.appendChild(a);
			a.click();
			document.body.removeChild(a);
			URL.revokeObjectURL(url);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Download failed');
		}
	}

	function isText(): boolean {
		const ct = metadata?.contentType ?? '';
		return (
			ct.startsWith('text/') ||
			ct.includes('json') ||
			ct.includes('xml') ||
			ct.includes('javascript') ||
			ct.includes('yaml')
		);
	}

	function isImage(): boolean {
		return metadata?.contentType?.startsWith('image/') ?? false;
	}
</script>

<Sheet bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<SheetContent class="flex w-full flex-col gap-0 p-0 sm:max-w-lg">
		<SheetHeader class="border-b border-border p-4">
			<SheetTitle class="font-mono text-sm break-all">
				{object?.key ?? ''}
			</SheetTitle>
			<SheetDescription>Object in {bucket ?? ''}</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
			<section>
				<h3 class="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
					Properties
				</h3>
				<dl class="grid grid-cols-[120px_1fr] gap-y-1.5 text-xs">
					<dt class="text-muted-foreground">Size</dt>
					<dd class="font-mono">{object ? formatBytes(object.size) : '—'}</dd>

					<dt class="text-muted-foreground">Modified</dt>
					<dd class="font-mono">{object ? formatTimestamp(object.lastModified) : '—'}</dd>

					<dt class="text-muted-foreground">Storage</dt>
					<dd>
						<Badge variant="outline">{object?.storageClass ?? 'STANDARD'}</Badge>
					</dd>

					<dt class="text-muted-foreground">ETag</dt>
					<dd class="truncate font-mono text-[11px]" title={object?.etag}>
						{object?.etag ?? '—'}
					</dd>

					{#if metaLoading}
						<dt class="text-muted-foreground">Metadata</dt>
						<dd>
							<Loader2 class="size-3 animate-spin" />
						</dd>
					{:else if metadata}
						<dt class="text-muted-foreground">Type</dt>
						<dd class="font-mono">{metadata.contentType ?? '—'}</dd>

						{#if metadata.versionId}
							<dt class="text-muted-foreground">Version</dt>
							<dd class="font-mono text-[11px]">{metadata.versionId}</dd>
						{/if}
					{/if}
				</dl>

				{#if metadata && Object.keys(metadata.metadata).length > 0}
					<h4 class="mt-4 mb-1 text-xs font-medium text-muted-foreground">User metadata</h4>
					<dl class="grid grid-cols-[120px_1fr] gap-y-1 text-xs">
						{#each Object.entries(metadata.metadata) as [k, v] (k)}
							<dt class="font-mono text-muted-foreground">{k}</dt>
							<dd class="truncate font-mono">{v}</dd>
						{/each}
					</dl>
				{/if}
			</section>

			<section>
				<h3 class="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
					Preview
				</h3>
				{#if isImage()}
					{#if imageBlobUrl}
						<img
							src={imageBlobUrl}
							alt={object?.key ?? 'object preview'}
							class="max-h-72 w-auto rounded-md border border-border object-contain"
						/>
					{:else}
						<Button variant="outline" size="sm" onclick={loadImagePreview} disabled={previewLoading}>
							{#if previewLoading}
								<Loader2 class="size-3.5 animate-spin" />
							{:else}
								<Eye class="size-3.5" />
							{/if}
							Load image preview
						</Button>
					{/if}
				{:else if preview !== null}
					<pre
						class="max-h-72 overflow-auto rounded-md border border-border bg-muted/40 p-2 font-mono text-[11px]">{preview}</pre>
				{:else if previewError}
					<p class="text-xs text-destructive">{previewError}</p>
				{:else}
					<Button
						variant="outline"
						size="sm"
						onclick={loadPreview}
						disabled={previewLoading || !isText()}
					>
						{#if previewLoading}
							<Loader2 class="size-3.5 animate-spin" />
						{:else}
							<Eye class="size-3.5" />
						{/if}
						{isText() ? 'Load text preview' : 'Preview unavailable for this content type'}
					</Button>
				{/if}
			</section>
		</div>

		<footer class="flex shrink-0 items-center justify-between gap-2 border-t border-border p-4">
			<Button variant="destructive" size="sm" onclick={() => object && onDelete(object)}>
				<Trash2 class="size-3.5" />
				Delete
			</Button>
			<Button variant="outline" size="sm" onclick={downloadFile} disabled={!bucket || !object}>
				<Download class="size-3.5" />
				Download
			</Button>
		</footer>
	</SheetContent>
</Sheet>
