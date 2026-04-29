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
	import { Textarea } from '$lib/components/ui/textarea';
	import { toast } from 'svelte-sonner';
	import { createPipe } from '$lib/api/pipes';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let source = $state('');
	let target = $state('');
	let roleArn = $state('arn:aws:iam::000000000000:role/PipesRole');
	let description = $state('');
	let batchSize = $state('10');
	let filterPatternJson = $state('');
	let enrichment = $state('');
	let creating = $state(false);

	function reset() {
		name = '';
		source = '';
		target = '';
		description = '';
		filterPatternJson = '';
		enrichment = '';
		batchSize = '10';
	}

	async function submit() {
		if (!name.trim()) return toast.error('Pipe name is required.');
		if (!source.trim()) return toast.error('Source ARN is required.');
		if (!target.trim()) return toast.error('Target ARN is required.');

		const sourceParameters: Record<string, unknown> = {};
		const sizeNum = parseInt(batchSize, 10);
		if (Number.isFinite(sizeNum) && sizeNum > 0 && source.includes(':sqs:')) {
			sourceParameters.SqsQueueParameters = { BatchSize: sizeNum };
		}
		if (filterPatternJson.trim()) {
			try {
				JSON.parse(filterPatternJson);
			} catch {
				return toast.error('Filter pattern must be valid JSON.');
			}
			sourceParameters.FilterCriteria = {
				Filters: [{ Pattern: filterPatternJson.trim() }]
			};
		}

		creating = true;
		try {
			await createPipe({
				name: name.trim(),
				source: source.trim(),
				target: target.trim(),
				roleArn: roleArn.trim(),
				description: description.trim() || undefined,
				desiredState: 'RUNNING',
				sourceParameters: Object.keys(sourceParameters).length > 0 ? sourceParameters : undefined,
				enrichment: enrichment.trim() || undefined
			});
			toast.success(`Pipe ${name.trim()} created.`);
			reset();
			onCreated?.();
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create pipe');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} onOpenChange={onOpenChange}>
	<DialogContent class="max-w-xl">
		<DialogHeader>
			<DialogTitle>New pipe</DialogTitle>
			<DialogDescription>
				Connect an SQS source to a Lambda, Step Functions, SQS, or SNS target.
			</DialogDescription>
		</DialogHeader>

		<div class="space-y-3">
			<div class="grid grid-cols-2 gap-3">
				<div class="space-y-1.5">
					<Label for="pipe-name">Name</Label>
					<Input id="pipe-name" bind:value={name} placeholder="orders-pipe" />
				</div>
				<div class="space-y-1.5">
					<Label for="pipe-role">Role ARN</Label>
					<Input id="pipe-role" bind:value={roleArn} class="font-mono text-xs" />
				</div>
			</div>

			<div class="space-y-1.5">
				<Label for="pipe-source">Source ARN</Label>
				<Input
					id="pipe-source"
					bind:value={source}
					placeholder="arn:aws:sqs:us-east-1:000000000000:my-queue"
					class="font-mono text-xs"
				/>
				<p class="text-[11px] text-muted-foreground">
					AWSim's pipes runner currently polls SQS sources only.
				</p>
			</div>

			<div class="space-y-1.5">
				<Label for="pipe-target">Target ARN</Label>
				<Input
					id="pipe-target"
					bind:value={target}
					placeholder="arn:aws:lambda:us-east-1:000000000000:function:processor"
					class="font-mono text-xs"
				/>
				<p class="text-[11px] text-muted-foreground">
					Supported targets: Lambda function, Step Functions state machine, SQS queue, SNS topic.
				</p>
			</div>

			<div class="grid grid-cols-2 gap-3">
				<div class="space-y-1.5">
					<Label for="pipe-batch">Batch size</Label>
					<Input id="pipe-batch" bind:value={batchSize} type="number" min="1" max="10000" />
				</div>
				<div class="space-y-1.5">
					<Label for="pipe-enrichment">Enrichment ARN <span class="text-muted-foreground">(optional)</span></Label>
					<Input
						id="pipe-enrichment"
						bind:value={enrichment}
						placeholder="arn:aws:lambda:...:function:transform"
						class="font-mono text-xs"
					/>
				</div>
			</div>

			<div class="space-y-1.5">
				<Label for="pipe-filter">Filter pattern <span class="text-muted-foreground">(optional JSON)</span></Label>
				<Textarea
					id="pipe-filter"
					bind:value={filterPatternJson}
					rows={3}
					placeholder={'{"body":{"type":["new-order"]}}'}
					class="font-mono text-xs"
				/>
			</div>

			<div class="space-y-1.5">
				<Label for="pipe-desc">Description</Label>
				<Input id="pipe-desc" bind:value={description} />
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)} disabled={creating}>
				Cancel
			</Button>
			<Button onclick={submit} disabled={creating}>
				{creating ? 'Creating…' : 'Create pipe'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
