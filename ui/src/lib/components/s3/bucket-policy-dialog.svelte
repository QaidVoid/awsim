<script lang="ts">
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Label } from '$lib/components/ui/label';
	import { getBucketPolicy, putBucketPolicy, deleteBucketPolicy } from '$lib/api/s3';
	import { toast } from 'svelte-sonner';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		open: boolean;
		bucket: string | null;
		onClose: () => void;
	}

	let { open = $bindable(false), bucket, onClose }: Props = $props();

	let policyText = $state('');
	let loading = $state(false);
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (open && bucket) {
			void loadPolicy(bucket);
		} else if (!open) {
			policyText = '';
			error = null;
		}
	});

	async function loadPolicy(b: string) {
		loading = true;
		error = null;
		try {
			const result = await getBucketPolicy(b);
			policyText = result.policy ? prettify(result.policy) : defaultPolicy(b);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load policy';
			policyText = defaultPolicy(b);
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
		if (!bucket) return;
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
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Failed to save policy';
			error = msg;
			toast.error(msg);
		} finally {
			saving = false;
		}
	}

	async function clearPolicy() {
		if (!bucket) return;
		saving = true;
		error = null;
		try {
			await deleteBucketPolicy(bucket);
			toast.success(`Policy removed from ${bucket}`);
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Failed to delete policy';
			error = msg;
			toast.error(msg);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<DialogContent class="sm:max-w-2xl">
		<DialogHeader>
			<DialogTitle>Bucket policy</DialogTitle>
			<DialogDescription>
				JSON policy applied to <span class="font-mono text-foreground">{bucket}</span>.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-2">
			<Label for="policy-text">Policy JSON</Label>
			{#if loading}
				<div class="flex h-48 items-center justify-center rounded-md border border-border">
					<Loader2 class="size-4 animate-spin text-muted-foreground" />
				</div>
			{:else}
				<Textarea
					id="policy-text"
					bind:value={policyText}
					rows={16}
					class="font-mono text-xs"
				/>
			{/if}
			{#if error}
				<p class="text-xs text-destructive">{error}</p>
			{/if}
		</div>

		<DialogFooter class="gap-2 sm:justify-between">
			<Button variant="destructive" onclick={clearPolicy} disabled={saving || loading}>
				Remove policy
			</Button>
			<div class="flex gap-2">
				<Button variant="outline" onclick={onClose} disabled={saving}>Cancel</Button>
				<Button onclick={save} disabled={saving || loading}>
					{#if saving}
						<Loader2 class="size-3.5 animate-spin" />
					{/if}
					Save policy
				</Button>
			</div>
		</DialogFooter>
	</DialogContent>
</Dialog>
