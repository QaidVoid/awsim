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
	import { createTopic } from '$lib/api/sns';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: (arn: string) => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let fifo = $state(false);
	let contentDedup = $state(false);
	let displayName = $state('');
	let kmsKeyId = $state('');
	let policy = $state('');
	let tagsText = $state('');
	let creating = $state(false);

	function reset() {
		name = '';
		fifo = false;
		contentDedup = false;
		displayName = '';
		kmsKeyId = '';
		policy = '';
		tagsText = '';
	}

	/** Parse `key=value` lines into a tag map, rejecting malformed rows. */
	function parseTags(): Record<string, string> | null {
		const tags: Record<string, string> = {};
		for (const line of tagsText.split('\n')) {
			const trimmed = line.trim();
			if (!trimmed) continue;
			const eq = trimmed.indexOf('=');
			if (eq <= 0) return null;
			tags[trimmed.slice(0, eq).trim()] = trimmed.slice(eq + 1).trim();
		}
		return tags;
	}

	async function submit() {
		if (!name.trim()) {
			toast.error('Topic name is required.');
			return;
		}
		const tags = parseTags();
		if (tags === null) {
			toast.error('Tags must be one `key=value` per line.');
			return;
		}
		// Catch a malformed policy here rather than after the topic exists.
		if (policy.trim()) {
			try {
				JSON.parse(policy);
			} catch {
				toast.error('Access policy must be valid JSON.');
				return;
			}
		}
		creating = true;
		try {
			const res = await createTopic(name.trim(), {
				fifo,
				contentBasedDeduplication: fifo ? contentDedup : false,
				displayName: displayName.trim() || undefined,
				kmsMasterKeyId: kmsKeyId.trim() || undefined,
				policy: policy.trim() || undefined,
				tags,
			});
			toast.success('Topic created.');
			reset();
			onOpenChange(false);
			onCreated?.(res.topicArn);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create topic');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-lg max-h-[85vh] overflow-y-auto">
		<DialogHeader>
			<DialogTitle>New SNS topic</DialogTitle>
			<DialogDescription>
				Topics fan out a published message to every subscriber.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="sns-create-name">Topic name</Label>
				<Input
					id="sns-create-name"
					bind:value={name}
					placeholder="my-topic"
					autocomplete="off"
				/>
			</div>

			<div class="flex flex-col gap-1">
				<Label for="sns-create-display">Display name</Label>
				<Input
					id="sns-create-display"
					bind:value={displayName}
					placeholder="Order events"
					autocomplete="off"
				/>
				<p class="text-[11px] text-muted-foreground">
					Shown to subscribers and used as the sender on SMS.
				</p>
			</div>

			<div class="flex items-center justify-between rounded-md border border-border px-3 py-2">
				<div>
					<Label for="sns-create-fifo" class="text-sm">FIFO topic</Label>
					<p class="text-[11px] text-muted-foreground">
						Strict ordering, no duplicates. Subscribers must be FIFO SQS queues.
					</p>
				</div>
				<Switch id="sns-create-fifo" bind:checked={fifo} />
			</div>

			{#if fifo}
				<div class="flex items-center justify-between rounded-md border border-border px-3 py-2">
					<div>
						<Label for="sns-create-dedup" class="text-sm">Content-based dedup</Label>
						<p class="text-[11px] text-muted-foreground">
							Hash the body instead of requiring a deduplication id.
						</p>
					</div>
					<Switch id="sns-create-dedup" bind:checked={contentDedup} />
				</div>
			{/if}

			<div class="flex flex-col gap-1">
				<Label for="sns-create-kms">Encryption key (optional)</Label>
				<Input
					id="sns-create-kms"
					bind:value={kmsKeyId}
					placeholder="alias/aws/sns"
					autocomplete="off"
				/>
				<p class="text-[11px] text-muted-foreground">
					KMS key id, ARN, or alias. The key must already exist.
				</p>
			</div>

			<div class="flex flex-col gap-1">
				<Label for="sns-create-policy">Access policy (optional)</Label>
				<textarea
					id="sns-create-policy"
					bind:value={policy}
					rows="4"
					placeholder={'{"Version":"2012-10-17","Statement":[]}'}
					class="rounded-md border border-border bg-background px-2 py-1.5 text-sm font-mono"
				></textarea>
			</div>

			<div class="flex flex-col gap-1">
				<Label for="sns-create-tags">Tags</Label>
				<textarea
					id="sns-create-tags"
					bind:value={tagsText}
					rows="3"
					placeholder="env=dev&#10;team=platform"
					class="rounded-md border border-border bg-background px-2 py-1.5 text-sm font-mono"
				></textarea>
				<p class="text-[11px] text-muted-foreground">One `key=value` per line.</p>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !name.trim()}>
				{creating ? 'Creating...' : 'Create topic'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
