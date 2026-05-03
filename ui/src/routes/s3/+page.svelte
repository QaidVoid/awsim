<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { afterNavigate, replaceState } from '$app/navigation';
	import { browser } from '$app/environment';
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
	import { useTab } from '$lib/util/tab.svelte';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import BucketList from '$lib/components/s3/bucket-list.svelte';
	import ObjectBrowser from '$lib/components/s3/object-browser.svelte';
	import BucketPropertiesTab from '$lib/components/s3/bucket-properties-tab.svelte';
	import BucketCorsTab from '$lib/components/s3/bucket-cors-tab.svelte';
	import BucketPolicyTab from '$lib/components/s3/bucket-policy-tab.svelte';
	import UploadZone from '$lib/components/s3/upload-zone.svelte';
	import ObjectDetailSheet from '$lib/components/s3/object-detail-sheet.svelte';
	import CreateBucketDialog from '$lib/components/s3/create-bucket-dialog.svelte';
	import ConfirmDialog from '$lib/components/s3/confirm-dialog.svelte';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';

	let buckets = $state<Bucket[]>([]);
	let bucketsLoading = $state(true);

	let selectedBucket = $state<Bucket | null>(null);
	let prefix = $state('');
	let objects = $state<S3Object[]>([]);
	let commonPrefixes = $state<S3CommonPrefix[]>([]);
	let objectsLoading = $state(false);

	let pageStack = $state<(string | undefined)[]>([]);
	let currentToken = $state<string | undefined>(undefined);
	let nextToken = $state<string | undefined>(undefined);
	let hasMore = $derived(nextToken !== undefined);

	let bucketFilter = $state('');
	let createOpen = $state(false);
	let detailOpen = $state(false);
	let detailObject = $state<S3Object | null>(null);

	let confirmBucketOpen = $state(false);
	let confirmBucketBusy = $state(false);
	let confirmObjectOpen = $state(false);
	let confirmObjectBusy = $state(false);
	let confirmObject = $state<S3Object | null>(null);

	let uploading = $state(false);

	let active: string = $state(
		useTab('s3', ['objects', 'properties', 'policy', 'cors'] as const, 'objects', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);

	let routerReady = $state(false);
	afterNavigate(() => {
		routerReady = true;
	});

	$effect(() => {
		if (!browser || !routerReady) return;
		const url = new URL(window.location.href);
		const name = selectedBucket?.name;
		const changed =
			url.searchParams.get('bucket') !== (name ?? null) ||
			url.searchParams.get('prefix') !== (prefix || null);
		if (!changed) return;
		if (name) {
			url.searchParams.set('bucket', name);
			if (prefix) url.searchParams.set('prefix', prefix);
			else url.searchParams.delete('prefix');
		} else {
			url.searchParams.delete('bucket');
			url.searchParams.delete('prefix');
		}
		replaceState(url.toString(), {});
	});

	onMount(() => {
		void loadBuckets().then(() => {
			const bucketName = page.url.searchParams.get('bucket');
			if (bucketName) {
				const b = buckets.find((x) => x.name === bucketName);
				if (b) {
					const pfx = page.url.searchParams.get('prefix') ?? '';
					void openBucket(b, pfx);
				}
			}
		});
	});

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

	async function openBucket(b: Bucket, initialPrefix = '') {
		selectedBucket = b;
		prefix = initialPrefix;
		pageStack = [];
		currentToken = undefined;
		active = 'objects';
		await fetchObjects(b.name, initialPrefix, undefined);
	}

	async function fetchObjects(bucket: string, pfx: string, token: string | undefined) {
		objectsLoading = true;
		try {
			const res = await listObjects(bucket, pfx, '/', token);
			objects = res.objects;
			commonPrefixes = res.commonPrefixes;
			nextToken = res.nextContinuationToken;
			currentToken = token;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to list objects');
			objects = [];
			commonPrefixes = [];
		} finally {
			objectsLoading = false;
		}
	}

	async function nextPage() {
		if (!nextToken || !selectedBucket) return;
		pageStack = [...pageStack, currentToken];
		await fetchObjects(selectedBucket.name, prefix, nextToken);
	}

	async function prevPage() {
		if (pageStack.length === 0 || !selectedBucket) return;
		const newStack = [...pageStack];
		const prevKey = newStack.pop();
		pageStack = newStack;
		await fetchObjects(selectedBucket.name, prefix, prevKey);
	}

	function navigatePrefix(newPrefix: string) {
		prefix = newPrefix;
		pageStack = [];
		currentToken = undefined;
		if (selectedBucket) void fetchObjects(selectedBucket.name, newPrefix, undefined);
	}

	function refreshObjects() {
		if (selectedBucket) void fetchObjects(selectedBucket.name, prefix, currentToken);
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
			void fetchObjects(selectedBucket.name, prefix, currentToken);
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
			void fetchObjects(bucket, prefix, currentToken);
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
					<Button
						variant="ghost"
						size="sm"
						onclick={() => (confirmBucketOpen = true)}
					>
						<Trash2 class="size-3.5 text-destructive" />
						Delete
					</Button>
				</div>

				<Tabs bind:value={active} class="flex min-h-0 min-w-0 flex-1 flex-col gap-0">
					<TabsList class="mx-4 mt-2 self-start">
						<TabsTrigger value="objects">Objects</TabsTrigger>
						<TabsTrigger value="properties">Properties</TabsTrigger>
						<TabsTrigger value="policy">Policy</TabsTrigger>
						<TabsTrigger value="cors">CORS</TabsTrigger>
					</TabsList>

					<div class="min-h-0 min-w-0 flex-1">
						<TabsContent value="objects" class="m-0 h-full min-w-0">
							<div class="flex h-full flex-col">
								<div class="min-h-0 flex-1">
									<ObjectBrowser
										bucket={selectedBucket.name}
										{prefix}
										{objects}
										{commonPrefixes}
										loading={objectsLoading}
										hasPrev={pageStack.length > 0}
										hasMore={hasMore}
										onNavigate={navigatePrefix}
										onSelectObject={selectObject}
										onDeleteObject={askDeleteObject}
										onRefresh={refreshObjects}
										onPrevPage={prevPage}
										onNextPage={nextPage}
									/>
								</div>
								<UploadZone {uploading} onFiles={uploadFiles} disabled={uploading} />
							</div>
						</TabsContent>
						<TabsContent value="properties" class="m-0 h-full min-w-0">
							<BucketPropertiesTab bucket={selectedBucket.name} />
						</TabsContent>
						<TabsContent value="policy" class="m-0 h-full min-w-0">
							<BucketPolicyTab bucket={selectedBucket.name} />
						</TabsContent>
						<TabsContent value="cors" class="m-0 h-full min-w-0">
							<BucketCorsTab bucket={selectedBucket.name} />
						</TabsContent>
					</div>
				</Tabs>
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
