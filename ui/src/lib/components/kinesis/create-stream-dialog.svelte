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
	import { toast } from 'svelte-sonner';
	import { createStream } from '$lib/api/kinesis';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let shardCount = $state(1);
	let streamMode = $state<'PROVISIONED' | 'ON_DEMAND'>('PROVISIONED');
	let creating = $state(false);

	async function submit() {
		if (!name.trim()) {
			toast.error('Stream name required.');
			return;
		}
		creating = true;
		try {
			await createStream(name.trim(), shardCount, streamMode);
			toast.success('Stream created.');
			name = '';
			shardCount = 1;
			streamMode = 'PROVISIONED';
			onOpenChange(false);
			onCreated?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create stream');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New Kinesis stream</DialogTitle>
			<DialogDescription>
				Each shard supports up to 1000 records/sec ingest and 5 reads/sec.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="kin-create-name">Stream name</Label>
				<Input id="kin-create-name" bind:value={name} placeholder="my-stream" />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="kin-create-mode">Capacity mode</Label>
				<select
					id="kin-create-mode"
					bind:value={streamMode}
					class="h-9 rounded-md border border-border bg-background px-2 text-sm"
				>
					<option value="PROVISIONED">Provisioned</option>
					<option value="ON_DEMAND">On-demand</option>
				</select>
			</div>
			{#if streamMode === 'PROVISIONED'}
				<div class="flex flex-col gap-1">
					<Label for="kin-create-shards">Shard count</Label>
					<Input
						id="kin-create-shards"
						type="number"
						min="1"
						max="10000"
						bind:value={shardCount}
					/>
				</div>
			{:else}
				<p class="text-[11px] text-muted-foreground">
					On-demand streams scale their own shards, starting at four.
				</p>
			{/if}
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !name.trim()}>
				{creating ? 'Creating...' : 'Create stream'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
