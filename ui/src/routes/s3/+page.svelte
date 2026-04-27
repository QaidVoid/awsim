<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		listBuckets,
		listObjects,
		deleteBucket,
		deleteObject,
		putObject,
		type Bucket,
		type S3Object,
		type S3CommonPrefix
	} from '$lib/api/s3';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import BucketList from '$lib/components/s3/bucket-list.svelte';
	import ObjectBrowser from '$lib/components/s3/object-browser.svelte';
	import UploadZone from '$lib/components/s3/upload-zone.svelte';
	import ObjectDetailSheet from '$lib/components/s3/object-detail-sheet.svelte';
	import BucketPolicyDialog from '$lib/components/s3/bucket-policy-dialog.svelte';
	import CreateBucketDialog from '$lib/components/s3/create-bucket-dialog.svelte';
	import ConfirmDialog from '$lib/components/s3/confirm-dialog.svelte';
	import Plus from '@lucide/svelte/icons/plus';
	import Shield from '@lucide/svelte/icons/shield';
	import Trash2 from '@lucide/svelte/icons/trash-2';

	let buckets = $state<Bucket[]>([]);
	let bucketsLoading = $state(true);

	let selectedBucket = $state<Bucket | null>(null);
	let prefix = $state('');
	let objects = $state<S3Object[]>([]);
	let commonPrefixes = $state<S3CommonPrefix[]>([]);
	let objectsLoading = $state(false);

	let bucketFilter = $state('');
	let createOpen = $state(false);
	let policyOpen = $state(false);
	let detailOpen = $state(false);
	let detailObject = $state<S3Object | null>(null);

	let confirmBucketOpen = $state(false);
	let confirmBucketBusy = $state(false);
	let confirmObjectOpen = $state(false);
	let confirmObjectBusy = $state(false);
	let confirmObject = $state<S3Object | null>(null);

	let uploading = $state(false);

	onMount(loadBuckets);

	async function loadBuckets() {
		bucketsLoading = true;
		try {
			buckets = await listBuckets();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to list buckets');
		} finally {
			bucketsLoading = false;
		}
	}

	async function openBucket(b: Bucket) {
		selectedBucket = b;
		prefix = '';
		await loadObjects(b.name, '');
	}

	async function loadObjects(bucket: string, pfx: string) {
		objectsLoading = true;
		try {
			const res = await listObjects(bucket, pfx, '/');
			objects = res.objects;
			commonPrefixes = res.commonPrefixes;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to list objects');
			objects = [];
			commonPrefixes = [];
		} finally {
			objectsLoading = false;
		}
	}

	function navigatePrefix(newPrefix: string) {
		prefix = newPrefix;
		if (selectedBucket) void loadObjects(selectedBucket.name, newPrefix);
	}

	function refreshObjects() {
		if (selectedBucket) void loadObjects(selectedBucket.name, prefix);
	}

	function selectObject(obj: S3Object) {
		detailObject = obj;
		detailOpen = true;
	}

	function askDeleteObject(obj: S3Object) {
		confirmObject = obj;
		confirmObjectOpen = true;
	}

	async function confirmDeleteObject() {
		if (!selectedBucket || !confirmObject) return;
		confirmObjectBusy = true;
		try {
			await deleteObject(selectedBucket.name, confirmObject.key);
			toast.success(`Deleted ${confirmObject.key}`);
			confirmObjectOpen = false;
			detailOpen = false;
			confirmObject = null;
			void loadObjects(selectedBucket.name, prefix);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete object');
		} finally {
			confirmObjectBusy = false;
		}
	}

	async function confirmDeleteBucket() {
		if (!selectedBucket) return;
		const name = selectedBucket.name;
		confirmBucketBusy = true;
		try {
			await deleteBucket(name);
			toast.success(`Deleted bucket ${name}`);
			confirmBucketOpen = false;
			selectedBucket = null;
			objects = [];
			commonPrefixes = [];
			await loadBuckets();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete bucket');
		} finally {
			confirmBucketBusy = false;
		}
	}

	async function uploadFiles(files: File[]) {
		if (!selectedBucket) return;
		uploading = true;
		const bucket = selectedBucket.name;
		let success = 0;
		for (const file of files) {
			try {
				const key = prefix + file.name;
				await putObject(bucket, key, file, file.type || undefined);
				success++;
			} catch (e) {
				toast.error(
					`Upload of ${file.name} failed: ${e instanceof Error ? e.message : 'unknown error'}`
				);
			}
		}
		if (success > 0) {
			toast.success(`Uploaded ${success} file${success === 1 ? '' : 's'}`);
			void loadObjects(bucket, prefix);
		}
		uploading = false;
	}

	async function onBucketCreated(name: string) {
		await loadBuckets();
		const b = buckets.find((x) => x.name === name);
		if (b) void openBucket(b);
	}
</script>

<ServicePage
	title="S3"
	description="Simple Storage Service — buckets, objects, and policies."
>
	{#snippet actions()}
		<Badge variant="outline" class="font-mono">
			{buckets.length} bucket{buckets.length === 1 ? '' : 's'}
		</Badge>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<Plus class="size-3.5" />
			Create bucket
		</Button>
	{/snippet}

	<div class="grid h-full min-h-0 grid-cols-[280px_1fr] divide-x divide-border">
		<aside class="min-h-0 overflow-hidden">
			<BucketList
				{buckets}
				selectedName={selectedBucket?.name ?? null}
				loading={bucketsLoading}
				onSelect={openBucket}
				bind:filter={bucketFilter}
			/>
		</aside>

		<section class="flex min-h-0 flex-col">
			{#if selectedBucket}
				<div
					class="flex shrink-0 items-center justify-between gap-2 border-b border-border bg-background/40 px-4 py-2"
				>
					<div class="flex items-center gap-2">
						<span class="text-xs font-medium text-muted-foreground">Bucket</span>
						<span class="font-mono text-sm">{selectedBucket.name}</span>
					</div>
					<div class="flex items-center gap-1.5">
						<Button variant="ghost" size="sm" onclick={() => (policyOpen = true)}>
							<Shield class="size-3.5" />
							Policy
						</Button>
						<Button
							variant="ghost"
							size="sm"
							onclick={() => (confirmBucketOpen = true)}
						>
							<Trash2 class="size-3.5 text-destructive" />
							Delete
						</Button>
					</div>
				</div>

				<div class="min-h-0 flex-1">
					<ObjectBrowser
						bucket={selectedBucket.name}
						{prefix}
						{objects}
						{commonPrefixes}
						loading={objectsLoading}
						onNavigate={navigatePrefix}
						onSelectObject={selectObject}
						onDeleteObject={askDeleteObject}
						onRefresh={refreshObjects}
					/>
				</div>

				<UploadZone {uploading} onFiles={uploadFiles} disabled={uploading} />
			{:else}
				<div
					class="flex h-full items-center justify-center p-6 text-sm text-muted-foreground"
				>
					Select a bucket to browse objects.
				</div>
			{/if}
		</section>
	</div>
</ServicePage>

<CreateBucketDialog
	bind:open={createOpen}
	onClose={() => (createOpen = false)}
	onCreated={onBucketCreated}
/>

<BucketPolicyDialog
	bind:open={policyOpen}
	bucket={selectedBucket?.name ?? null}
	onClose={() => (policyOpen = false)}
/>

<ObjectDetailSheet
	bind:open={detailOpen}
	bucket={selectedBucket?.name ?? null}
	object={detailObject}
	onClose={() => (detailOpen = false)}
	onDelete={askDeleteObject}
/>

<ConfirmDialog
	bind:open={confirmBucketOpen}
	title="Delete bucket?"
	description={`This will permanently delete "${selectedBucket?.name}". The bucket must be empty.`}
	busy={confirmBucketBusy}
	onConfirm={confirmDeleteBucket}
	onClose={() => (confirmBucketOpen = false)}
/>

<ConfirmDialog
	bind:open={confirmObjectOpen}
	title="Delete object?"
	description={`Permanently delete "${confirmObject?.key ?? ''}".`}
	busy={confirmObjectBusy}
	onConfirm={confirmDeleteObject}
	onClose={() => (confirmObjectOpen = false)}
/>
