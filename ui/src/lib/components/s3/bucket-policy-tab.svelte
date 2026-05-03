<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		getBucketPolicy,
		putBucketPolicy,
		deleteBucketPolicy
	} from '$lib/api/s3';
	import { Button } from '$lib/components/ui/button';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Label } from '$lib/components/ui/label';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Save from '@lucide/svelte/icons/save';

	interface Props {
		bucket: string;
	}

	let { bucket }: Props = $props();

	let policyText = $state('');
	let loading = $state(true);
	let saving = $state(false);
	let error = $state<string | null>(null);

	onMount(loadPolicy);

	async function loadPolicy() {
		loading = true;
		error = null;
		try {
			const result = await getBucketPolicy(bucket);
			policyText = result.policy ? prettify(result.policy) : defaultPolicy(bucket);
		} catch {
			policyText = defaultPolicy(bucket);
		} finally {
			loading = false;
		}
	}

	function prettify(json: string): string {
		try {
			return JSON.stringify(JSON.parse(json), null, 2);
		} catch {
			return json;
		}
	}

	function defaultPolicy(b: string): string {
		return JSON.stringify(
			{
				Version: '2012-10-17',
				Statement: [
					{
						Sid: 'AllowPublicRead',
						Effect: 'Allow',
						Principal: '*',
						Action: ['s3:GetObject'],
						Resource: [`arn:aws:s3:::${b}/*`]
					}
				]
			},
			null,
			2
		);
	}

	async function save() {
		try {
			JSON.parse(policyText);
		} catch (e) {
			error = e instanceof Error ? `Invalid JSON: ${e.message}` : 'Invalid JSON';
			return;
		}
		saving = true;
		error = null;
		try {
			await putBucketPolicy(bucket, policyText);
			toast.success(`Policy saved for ${bucket}`);
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Failed to save policy';
			error = msg;
			toast.error(msg);
		} finally {
			saving = false;
		}
	}

	async function clearPolicy() {
		saving = true;
		error = null;
		try {
			await deleteBucketPolicy(bucket);
			toast.success(`Policy removed from ${bucket}`);
			policyText = defaultPolicy(bucket);
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Failed to delete policy';
			error = msg;
			toast.error(msg);
		} finally {
			saving = false;
		}
	}
</script>

<div class="h-full overflow-auto p-4">
	<div class="mx-auto max-w-2xl">
		{#if loading}
			<div class="flex items-center justify-center py-12 text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else}
			<div class="mb-3 flex items-center justify-between">
				<Label for="s3-policy-text">Bucket policy (JSON)</Label>
				<div class="flex gap-2">
					<Button size="sm" variant="destructive" onclick={clearPolicy} disabled={saving}>
						Remove policy
					</Button>
					<Button size="sm" onclick={save} disabled={saving || loading}>
						{#if saving}
							<Loader2 class="size-3.5 animate-spin" />
						{/if}
						<Save class="size-3.5" />
						Save
					</Button>
				</div>
			</div>
			<Textarea
				id="s3-policy-text"
				bind:value={policyText}
				rows={20}
				class="font-mono text-xs"
			/>
			{#if error}
				<p class="mt-2 text-xs text-destructive">{error}</p>
			{/if}
		{/if}
	</div>
</div>
