<script lang="ts">
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Switch } from '$lib/components/ui/switch';
	import { toast } from 'svelte-sonner';
	import { createDistribution } from '$lib/api/cloudfront';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let originDomain = $state('');
	let comment = $state('');
	let enabled = $state(true);
	let creating = $state(false);

	function reset() {
		originDomain = '';
		comment = '';
		enabled = true;
	}

	async function submit() {
		if (!originDomain.trim()) {
			toast.error('Origin domain is required.');
			return;
		}
		creating = true;
		try {
			await createDistribution({
				originDomain: originDomain.trim(),
				comment: comment.trim() || undefined,
				enabled,
			});
			toast.success('Distribution created.');
			reset();
			onOpenChange(false);
			onCreated?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create distribution');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New distribution</DialogTitle>
			<DialogDescription>
				Cache content from a single origin. Add behaviors and policies later.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="cf-create-origin">Origin domain</Label>
				<Input
					id="cf-create-origin"
					bind:value={originDomain}
					placeholder="example.s3.amazonaws.com"
					autocomplete="off"
				/>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="cf-create-comment">Comment (optional)</Label>
				<Input id="cf-create-comment" bind:value={comment} autocomplete="off" />
			</div>
			<div class="flex items-center justify-between rounded-md border border-border px-3 py-2">
				<Label for="cf-create-enabled" class="text-sm">Enabled</Label>
				<Switch id="cf-create-enabled" bind:checked={enabled} />
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !originDomain.trim()}>
				{creating ? 'Creating…' : 'Create distribution'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
