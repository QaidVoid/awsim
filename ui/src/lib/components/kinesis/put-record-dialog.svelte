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
	import { Textarea } from '$lib/components/ui/textarea';
	import { toast } from 'svelte-sonner';
	import { putRecord } from '$lib/api/kinesis';

	interface Props {
		open: boolean;
		streamName: string;
		onOpenChange: (open: boolean) => void;
		onSent?: () => void;
	}

	let { open, streamName, onOpenChange, onSent }: Props = $props();

	let partitionKey = $state('partition-1');
	let data = $state('{"hello":"world"}');
	let sending = $state(false);

	async function send() {
		if (!partitionKey.trim()) {
			toast.error('Partition key is required.');
			return;
		}
		if (!data.trim()) {
			toast.error('Data is required.');
			return;
		}
		sending = true;
		try {
			const res = await putRecord(streamName, data, partitionKey.trim());
			toast.success(`Sent to ${res.shardId}`);
			onSent?.();
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'PutRecord failed');
		} finally {
			sending = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle>Put record</DialogTitle>
			<DialogDescription>
				Publish a single record to <span class="font-mono">{streamName}</span>.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="kin-pr-partition">Partition key</Label>
				<Input id="kin-pr-partition" bind:value={partitionKey} />
				<p class="text-[11px] text-muted-foreground">
					Determines which shard receives the record.
				</p>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="kin-pr-data">Data (utf-8)</Label>
				<Textarea
					id="kin-pr-data"
					bind:value={data}
					rows={8}
					class="font-mono text-xs"
				/>
				<p class="text-[11px] text-muted-foreground">
					Encoded as base64 before transport.
				</p>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={send} disabled={sending}>
				{sending ? 'Sending…' : 'Put record'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
