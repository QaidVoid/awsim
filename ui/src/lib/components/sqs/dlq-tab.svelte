<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import ShieldAlertIcon from '@lucide/svelte/icons/shield-alert';
	import { toast } from 'svelte-sonner';
	import {
		redriveMessages,
		type Queue,
		type QueueAttributes,
	} from '$lib/api/sqs';

	interface Props {
		current: Queue;
		attrs: QueueAttributes;
		queues: Queue[];
		attrsByUrl: Record<string, QueueAttributes>;
		onRedriven?: () => void;
	}

	let { current, attrs, queues, attrsByUrl, onRedriven }: Props = $props();

	let redriving = $state(false);

	let dlqTargetUrl = $derived.by((): string | null => {
		if (!attrs.redrivePolicy) return null;
		const arn = attrs.redrivePolicy.deadLetterTargetArn;
		const dlqName = arn.split(':').pop();
		const target = queues.find((q) => q.name === dlqName);
		return target?.url ?? null;
	});

	let consumersWithThisAsDlq = $derived(
		queues.filter((q) => {
			const a = attrsByUrl[q.url];
			if (!a?.redrivePolicy) return false;
			const dlqName = a.redrivePolicy.deadLetterTargetArn.split(':').pop();
			return dlqName === current.name;
		})
	);

	async function handleRedrive(targetUrl: string) {
		const target = queues.find((q) => q.url === targetUrl);
		if (!target) return;
		redriving = true;
		try {
			const res = await redriveMessages(current.url, targetUrl);
			toast.success(`Redrove ${res.moved} message${res.moved === 1 ? '' : 's'} to ${target.name}`);
			onRedriven?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Redrive failed');
		} finally {
			redriving = false;
		}
	}
</script>

<div class="flex flex-col gap-4 p-4">
	{#if attrs.redrivePolicy}
		<section class="rounded-md border border-border bg-card/40 p-4">
			<h3 class="mb-3 text-sm font-semibold">Dead-letter routing</h3>
			<dl class="grid grid-cols-[140px_1fr] gap-x-4 gap-y-2 text-xs">
				<dt class="text-muted-foreground">Target ARN</dt>
				<dd class="font-mono text-[11px] break-all">
					{attrs.redrivePolicy.deadLetterTargetArn}
				</dd>
				<dt class="text-muted-foreground">Max receive count</dt>
				<dd>{attrs.redrivePolicy.maxReceiveCount}</dd>
			</dl>
			<p class="mt-3 text-xs text-muted-foreground">
				Messages received more than {attrs.redrivePolicy.maxReceiveCount} times on this queue
				will be moved to the dead-letter target automatically.
			</p>
		</section>
	{/if}

	{#if consumersWithThisAsDlq.length > 0}
		<section class="rounded-md border border-border bg-card/40 p-4">
			<div class="mb-3 flex items-center justify-between">
				<h3 class="text-sm font-semibold">This queue is a dead-letter target</h3>
				<Badge variant="outline" class="h-4 px-1.5 text-[10px]">
					{consumersWithThisAsDlq.length} source
					{consumersWithThisAsDlq.length === 1 ? '' : 's'}
				</Badge>
			</div>
			<p class="mb-3 text-xs text-muted-foreground">
				Redrive sweeps {attrs.approximateNumberOfMessages} visible message{attrs.approximateNumberOfMessages ===
				1
					? ''
					: 's'} back to a source queue.
			</p>
			<ul class="flex flex-col gap-2">
				{#each consumersWithThisAsDlq as source (source.url)}
					<li
						class="flex items-center justify-between gap-3 rounded-md border border-border bg-background/40 px-3 py-2"
					>
						<div class="min-w-0">
							<p class="truncate font-mono text-xs">{source.name}</p>
							<p class="text-[11px] text-muted-foreground">
								{attrsByUrl[source.url]?.approximateNumberOfMessages ?? 0} message(s) currently
							</p>
						</div>
						<Button
							size="xs"
							variant="outline"
							onclick={() => handleRedrive(source.url)}
							disabled={redriving || attrs.approximateNumberOfMessages === 0}
						>
							Redrive →
						</Button>
					</li>
				{/each}
			</ul>
		</section>
	{/if}

	{#if !attrs.redrivePolicy && consumersWithThisAsDlq.length === 0}
		{#if dlqTargetUrl}{/if}
		<EmptyState
			icon={ShieldAlertIcon}
			title="No DLQ wiring"
			description="This queue isn't configured to forward to a dead-letter queue, and nothing is configured to forward here."
		/>
	{/if}
</div>
