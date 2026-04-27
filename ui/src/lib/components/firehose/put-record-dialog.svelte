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
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Switch } from '$lib/components/ui/switch';
	import { toast } from 'svelte-sonner';
	import { putRecord, putRecordBatch } from '$lib/api/firehose';

	interface Props {
		open: boolean;
		streamName: string;
		onOpenChange: (open: boolean) => void;
	}

	let { open, streamName, onOpenChange }: Props = $props();

	let body = $state('{"hello":"firehose"}');
	let asBatch = $state(false);
	let sending = $state(false);

	async function send() {
		if (!body.trim()) {
			toast.error('Body is required.');
			return;
		}
		sending = true;
		try {
			if (asBatch) {
				const records = body
					.split('\n')
					.map((s) => s.trim())
					.filter(Boolean);
				if (records.length === 0) {
					toast.error('Provide at least one line.');
					return;
				}
				const res = await putRecordBatch(streamName, records);
				if (res.failedPutCount > 0) {
					toast.error(`Sent with ${res.failedPutCount} failure(s).`);
				} else {
					toast.success(`${records.length} record(s) sent.`);
					onOpenChange(false);
				}
			} else {
				await putRecord(streamName, body);
				toast.success('Record sent.');
				onOpenChange(false);
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Send failed');
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
				Publish to <span class="font-mono">{streamName}</span>. Records are buffered to the
				configured destination.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<label class="flex items-center gap-2 text-xs text-muted-foreground" for="fh-pr-batch">
				<Switch id="fh-pr-batch" bind:checked={asBatch} size="sm" />
				Batch mode (one record per line)
			</label>
			<div class="flex flex-col gap-1">
				<Label for="fh-pr-body">Data</Label>
				<Textarea id="fh-pr-body" bind:value={body} rows={8} class="font-mono text-xs" />
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={send} disabled={sending}>
				{sending ? 'Sending…' : asBatch ? 'Send batch' : 'Send'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
