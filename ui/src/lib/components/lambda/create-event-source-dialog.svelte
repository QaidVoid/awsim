<script lang="ts">
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { Textarea } from '$lib/components/ui/textarea';
	import { toast } from 'svelte-sonner';
	import { createEventSourceMapping } from '$lib/api/lambda';

	interface Props {
		open: boolean;
		functionName: string;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let { open, functionName, onOpenChange, onCreated }: Props = $props();

	let sourceArn = $state('');
	let batchSize = $state('10');
	let startingPosition = $state('TRIM_HORIZON');
	let batchingWindow = $state('0');
	let filterPattern = $state('');
	let dlqArn = $state('');
	let creating = $state(false);

	function reset() {
		sourceArn = '';
		batchSize = '10';
		startingPosition = 'TRIM_HORIZON';
		batchingWindow = '0';
		filterPattern = '';
		dlqArn = '';
	}

	function isStreamSource(arn: string): boolean {
		return arn.includes(':kinesis:') || arn.includes(':dynamodb:');
	}

	async function submit() {
		if (!sourceArn.trim()) return toast.error('Source ARN is required.');

		if (filterPattern.trim()) {
			try {
				JSON.parse(filterPattern);
			} catch {
				return toast.error('Filter pattern must be valid JSON.');
			}
		}

		const sizeNum = parseInt(batchSize, 10);
		const windowNum = parseInt(batchingWindow, 10);

		creating = true;
		try {
			await createEventSourceMapping({
				functionName,
				eventSourceArn: sourceArn.trim(),
				batchSize: Number.isFinite(sizeNum) && sizeNum > 0 ? sizeNum : undefined,
				maximumBatchingWindowInSeconds:
					Number.isFinite(windowNum) && windowNum > 0 ? windowNum : undefined,
				startingPosition: isStreamSource(sourceArn) ? startingPosition : undefined,
				filterPatternJson: filterPattern.trim() || undefined,
				destinationOnFailureArn: dlqArn.trim() || undefined
			});
			toast.success('Event source mapping created.');
			reset();
			onCreated?.();
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create mapping');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="max-w-xl">
		<DialogHeader>
			<DialogTitle>Add event source</DialogTitle>
			<DialogDescription>
				Trigger <span class="font-mono">{functionName}</span> from an SQS queue, Kinesis stream, or DynamoDB stream.
			</DialogDescription>
		</DialogHeader>

		<div class="space-y-3">
			<div class="space-y-1.5">
				<Label for="esm-source">Source ARN</Label>
				<Input
					id="esm-source"
					bind:value={sourceArn}
					placeholder="arn:aws:sqs:us-east-1:000000000000:my-queue"
					class="font-mono text-xs"
				/>
			</div>

			<div class="grid grid-cols-2 gap-3">
				<div class="space-y-1.5">
					<Label for="esm-batch">Batch size</Label>
					<Input id="esm-batch" bind:value={batchSize} type="number" min="1" max="10000" />
				</div>
				<div class="space-y-1.5">
					<Label for="esm-window">Batching window (s)</Label>
					<Input id="esm-window" bind:value={batchingWindow} type="number" min="0" max="300" />
				</div>
			</div>

			{#if isStreamSource(sourceArn)}
				<div class="space-y-1.5">
					<Label for="esm-start">Starting position</Label>
					<Select type="single" bind:value={startingPosition}>
						<SelectTrigger id="esm-start" class="w-full">
							{startingPosition}
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="TRIM_HORIZON" label="TRIM_HORIZON">TRIM_HORIZON</SelectItem>
							<SelectItem value="LATEST" label="LATEST">LATEST</SelectItem>
							<SelectItem value="AT_TIMESTAMP" label="AT_TIMESTAMP">AT_TIMESTAMP</SelectItem>
						</SelectContent>
					</Select>
				</div>
			{/if}

			<div class="space-y-1.5">
				<Label for="esm-filter">Filter pattern <span class="text-muted-foreground">(optional JSON)</span></Label>
				<Textarea
					id="esm-filter"
					bind:value={filterPattern}
					rows={3}
					placeholder={'{"body":{"type":["new"]}}'}
					class="font-mono text-xs"
				/>
				<p class="text-[11px] text-muted-foreground">
					EventBridge content-pattern syntax: equality arrays, <code>prefix</code>, <code>suffix</code>,
					<code>exists</code>, <code>anything-but</code>, <code>numeric</code>.
				</p>
			</div>

			<div class="space-y-1.5">
				<Label for="esm-dlq">On-failure destination ARN <span class="text-muted-foreground">(optional)</span></Label>
				<Input
					id="esm-dlq"
					bind:value={dlqArn}
					placeholder="arn:aws:sqs:us-east-1:000000000000:dlq"
					class="font-mono text-xs"
				/>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)} disabled={creating}>
				Cancel
			</Button>
			<Button onclick={submit} disabled={creating}>
				{creating ? 'Creating…' : 'Add source'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
