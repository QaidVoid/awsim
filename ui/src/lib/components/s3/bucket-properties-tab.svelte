<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		getBucketVersioning,
		putBucketVersioning,
		getBucketEncryption,
		putBucketEncryption,
		deleteBucketEncryption,
		getBucketTagging,
		putBucketTagging,
		type BucketTag
	} from '$lib/api/s3';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { Separator } from '$lib/components/ui/separator';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Save from '@lucide/svelte/icons/save';
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';

	interface Props {
		bucket: string;
	}

	let { bucket }: Props = $props();

	let loading = $state(true);
	let versioning = $state('');
	let encryption = $state({ enabled: false, algorithm: '' });
	let tags = $state<BucketTag[]>([]);
	let saving = $state(false);

	let newTagKey = $state('');
	let newTagValue = $state('');

	onMount(load);

	async function load() {
		loading = true;
		try {
			const [v, e, t] = await Promise.all([
				getBucketVersioning(bucket),
				getBucketEncryption(bucket),
				getBucketTagging(bucket)
			]);
			versioning = v.status;
			encryption = e;
			tags = t;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load properties');
		} finally {
			loading = false;
		}
	}

	async function toggleVersioning(enable: boolean) {
		saving = true;
		try {
			await putBucketVersioning(bucket, enable ? 'Enabled' : 'Suspended');
			versioning = enable ? 'Enabled' : 'Suspended';
			toast.success(`Versioning ${enable ? 'enabled' : 'suspended'}`);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed');
		} finally {
			saving = false;
		}
	}

	async function toggleEncryption(enable: boolean) {
		saving = true;
		try {
			if (enable) {
				await putBucketEncryption(bucket, 'AES256');
				encryption = { enabled: true, algorithm: 'AES256' };
			} else {
				await deleteBucketEncryption(bucket);
				encryption = { enabled: false, algorithm: '' };
			}
			toast.success(`Encryption ${enable ? 'enabled' : 'disabled'}`);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed');
		} finally {
			saving = false;
		}
	}

	function addTag() {
		if (!newTagKey.trim()) return;
		tags = [...tags, { key: newTagKey.trim(), value: newTagValue.trim() }];
		newTagKey = '';
		newTagValue = '';
	}

	function removeTag(index: number) {
		tags = tags.filter((_, i) => i !== index);
	}

	async function saveTags() {
		saving = true;
		try {
			await putBucketTagging(bucket, tags);
			toast.success('Tags saved');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to save tags');
		} finally {
			saving = false;
		}
	}
</script>

<div class="h-full overflow-auto p-4">
	{#if loading}
		<div class="flex items-center justify-center py-12 text-muted-foreground">
			<Loader2 class="size-4 animate-spin" />
		</div>
	{:else}
		<div class="mx-auto max-w-2xl space-y-6">
			<!-- Versioning -->
			<section>
				<h3 class="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
					Versioning
				</h3>
				<div class="flex items-center gap-3">
					<Badge variant={versioning === 'Enabled' ? 'default' : 'outline'}>
						{versioning || 'Disabled'}
					</Badge>
					<Button
						size="sm"
						variant={versioning === 'Enabled' ? 'outline' : 'default'}
						onclick={() => toggleVersioning(versioning !== 'Enabled')}
						disabled={saving}
					>
						{versioning === 'Enabled' ? 'Suspend' : 'Enable'}
					</Button>
				</div>
			</section>

			<Separator />

			<!-- Encryption -->
			<section>
				<h3 class="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
					Server-side encryption
				</h3>
				<div class="flex items-center gap-3">
					<Badge variant={encryption.enabled ? 'default' : 'outline'}>
						{encryption.enabled ? encryption.algorithm : 'Disabled'}
					</Badge>
					<Button
						size="sm"
						variant={encryption.enabled ? 'outline' : 'default'}
						onclick={() => toggleEncryption(!encryption.enabled)}
						disabled={saving}
					>
						{encryption.enabled ? 'Disable' : 'Enable AES256'}
					</Button>
				</div>
			</section>

			<Separator />

			<!-- Tags -->
			<section>
				<h3 class="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
					Tags
				</h3>

				{#if tags.length > 0}
					<div class="mb-3 space-y-1.5">
						{#each tags as tag, i (i)}
							<div class="flex items-center gap-2">
								<code class="rounded bg-muted px-1.5 py-0.5 text-xs">{tag.key}</code>
								<span class="text-muted-foreground">=</span>
								<code class="rounded bg-muted px-1.5 py-0.5 text-xs">{tag.value}</code>
								<Button variant="ghost" size="icon-xs" onclick={() => removeTag(i)}>
									<X class="size-3" />
								</Button>
							</div>
						{/each}
					</div>
				{/if}

				<div class="flex items-end gap-2">
					<div class="flex-1">
						<Label class="text-[11px]">Key</Label>
						<Input bind:value={newTagKey} placeholder="tag-key" class="h-8 text-xs" />
					</div>
					<div class="flex-1">
						<Label class="text-[11px]">Value</Label>
						<Input bind:value={newTagValue} placeholder="tag-value" class="h-8 text-xs" />
					</div>
					<Button size="sm" variant="outline" onclick={addTag} disabled={!newTagKey.trim()}>
						<Plus class="size-3.5" />
					</Button>
				</div>

				<Button size="sm" class="mt-3" onclick={saveTags} disabled={saving}>
					{#if saving}
						<Loader2 class="size-3.5 animate-spin" />
					{/if}
					<Save class="size-3.5" />
					Save tags
				</Button>
			</section>
		</div>
	{/if}
</div>
