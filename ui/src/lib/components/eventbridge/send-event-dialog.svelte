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
	import { putEvents } from '$lib/api/eventbridge';

	interface Props {
		open: boolean;
		busName: string;
		onOpenChange: (open: boolean) => void;
	}

	let { open, busName, onOpenChange }: Props = $props();

	let source = $state('my.app');
	let detailType = $state('MyEvent');
	let detail = $state(JSON.stringify({ key: 'value' }, null, 2));
	let resources = $state('');
	let sending = $state(false);

	async function send() {
		if (!source.trim() || !detailType.trim()) {
			toast.error('Source and detail-type are required.');
			return;
		}
		sending = true;
		try {
			const res = await putEvents([
				{
					source: source.trim(),
					detailType: detailType.trim(),
					detail,
					eventBusName: busName,
					resources: resources
						.split(',')
						.map((s) => s.trim())
						.filter(Boolean),
				},
			]);
			if (res.failedEntryCount > 0) {
				toast.error(`Event rejected (${res.failedEntryCount} failed entries)`);
			} else {
				toast.success(`Sent to ${busName}.`);
				onOpenChange(false);
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'PutEvents failed');
		} finally {
			sending = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle>Send event</DialogTitle>
			<DialogDescription>
				Publish a single event to <span class="font-mono">{busName}</span>.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="grid grid-cols-2 gap-3">
				<div class="flex flex-col gap-1">
					<Label for="evb-evt-source">Source</Label>
					<Input id="evb-evt-source" bind:value={source} />
				</div>
				<div class="flex flex-col gap-1">
					<Label for="evb-evt-type">Detail type</Label>
					<Input id="evb-evt-type" bind:value={detailType} />
				</div>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="evb-evt-resources">Resources (comma-separated ARNs)</Label>
				<Input id="evb-evt-resources" bind:value={resources} placeholder="optional" />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="evb-evt-detail">Detail (JSON)</Label>
				<Textarea
					id="evb-evt-detail"
					bind:value={detail}
					rows={8}
					class="font-mono text-xs"
				/>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={send} disabled={sending}>
				{sending ? 'Sending…' : 'Send event'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
