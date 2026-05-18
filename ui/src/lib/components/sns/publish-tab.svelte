<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Switch } from '$lib/components/ui/switch';
	import SendIcon from '@lucide/svelte/icons/send';
	import Share2 from '@lucide/svelte/icons/share-2';
	import { toast } from 'svelte-sonner';
	import { publish, listSubscriptionsByTopic, type Subscription } from '$lib/api/sns';

	interface Props {
		topicArn: string;
		isFifo: boolean;
		/** Jump the page to this topic's Subscriptions tab. */
		onViewSubscriptions?: () => void;
	}

	let { topicArn, isFifo, onViewSubscriptions }: Props = $props();

	interface PubRun {
		id: number;
		ts: number;
		subject?: string;
		message: string;
		messageId: string;
		fanout: Subscription[];
	}

	let subject = $state('');
	let message = $state('');
	let asJson = $state(false);
	let messageGroupId = $state('default');
	let messageDeduplicationId = $state('');
	let publishing = $state(false);
	let history = $state<PubRun[]>([]);
	let lastArn = $state('');
	let nextId = 0;

	$effect(() => {
		if (topicArn !== lastArn) {
			lastArn = topicArn;
			history = [];
		}
	});

	function relTime(ts: number): string {
		const s = Math.max(0, Math.round((Date.now() - ts) / 1000));
		if (s < 60) return `${s}s ago`;
		if (s < 3600) return `${Math.floor(s / 60)}m ago`;
		return `${Math.floor(s / 3600)}h ago`;
	}

	async function send() {
		if (!message.trim()) {
			toast.error('Message body cannot be empty.');
			return;
		}
		publishing = true;
		try {
			const res = await publish({
				topicArn,
				message,
				subject: subject.trim() || undefined,
				messageStructure: asJson ? 'json' : undefined,
				messageGroupId: isFifo ? messageGroupId.trim() || 'default' : undefined,
				messageDeduplicationId:
					isFifo && messageDeduplicationId.trim() ? messageDeduplicationId.trim() : undefined,
			});
			// Snapshot who it fanned out to at publish time.
			const fanout = await listSubscriptionsByTopic(topicArn).catch(() => []);
			const run: PubRun = {
				id: nextId++,
				ts: Date.now(),
				subject: subject.trim() || undefined,
				message,
				messageId: res.messageId,
				fanout
			};
			history = [run, ...history].slice(0, 20);
			toast.success(
				`Published to ${fanout.length} subscriber${fanout.length === 1 ? '' : 's'}.`
			);
			message = '';
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Publish failed');
		} finally {
			publishing = false;
		}
	}

	function reuse(run: PubRun) {
		message = run.message;
		subject = run.subject ?? '';
	}

	function loadJsonSample() {
		message = JSON.stringify(
			{
				default: 'plain default message',
				email: 'rich email message',
				sqs: '{"event":"order.created"}',
			},
			null,
			2
		);
		asJson = true;
	}
</script>

<div class="flex flex-col gap-3 p-4">
	{#if history.length}
		<section class="rounded-md border border-border bg-card">
			<header
				class="flex items-center justify-between border-b border-border px-3 py-2"
			>
				<h3 class="text-xs font-medium uppercase tracking-wide text-muted-foreground">
					Published this session ({history.length})
				</h3>
				{#if onViewSubscriptions}
					<button
						type="button"
						class="flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground"
						onclick={onViewSubscriptions}
					>
						<Share2 class="size-3" />
						View subscribers
					</button>
				{/if}
			</header>
			<div class="flex flex-col divide-y divide-border/60">
				{#each history as run (run.id)}
					{@const fanned = run.fanout.length}
					<button
						type="button"
						onclick={() => reuse(run)}
						class="flex items-center gap-3 px-3 py-1.5 text-left text-xs hover:bg-muted/50"
						title={fanned
							? `Fanned out to: ${run.fanout.map((s) => `${s.protocol}:${s.endpoint}`).join(', ')}`
							: 'No subscribers at publish time'}
					>
						<span
							class="size-1.5 shrink-0 rounded-full {fanned
								? 'bg-emerald-500'
								: 'bg-amber-500'}"
						></span>
						<span class="min-w-0 flex-1 truncate font-mono">
							{(run.subject ? `[${run.subject}] ` : '') +
								run.message.replace(/\s+/g, ' ').trim()}
						</span>
						<span class="shrink-0 text-muted-foreground">
							-&gt; {fanned} sub{fanned === 1 ? '' : 's'}
						</span>
						<span class="shrink-0 text-muted-foreground">{relTime(run.ts)}</span>
					</button>
				{/each}
			</div>
		</section>
	{/if}

	<div class="flex flex-col gap-1">
		<Label for="sns-pub-subject">Subject (optional)</Label>
		<Input id="sns-pub-subject" bind:value={subject} maxlength={100} />
	</div>

	<div class="flex flex-col gap-2">
		<div class="flex items-center justify-between">
			<Label for="sns-pub-body">Message body</Label>
			<div class="flex items-center gap-3">
				<label class="flex items-center gap-2 text-xs text-muted-foreground" for="sns-pub-json">
					<Switch id="sns-pub-json" bind:checked={asJson} size="sm" />
					Per-protocol JSON
				</label>
				<Button variant="ghost" size="xs" onclick={loadJsonSample}>Load sample</Button>
			</div>
		</div>
		<Textarea
			id="sns-pub-body"
			bind:value={message}
			rows={10}
			class="font-mono text-xs"
			placeholder={asJson
				? '{"default": "...", "sqs": "..."}'
				: 'Plain text or JSON…'}
		/>
	</div>

	{#if isFifo}
		<div class="grid grid-cols-2 gap-3">
			<div class="flex flex-col gap-1">
				<Label for="sns-pub-group">Message group id</Label>
				<Input id="sns-pub-group" bind:value={messageGroupId} />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="sns-pub-dedup">Deduplication id</Label>
				<Input id="sns-pub-dedup" bind:value={messageDeduplicationId} placeholder="optional" />
			</div>
		</div>
	{/if}

	<div class="flex items-center justify-end gap-2 pt-2">
		{#if onViewSubscriptions && history.length}
			<Button variant="outline" onclick={onViewSubscriptions}>
				<Share2 />
				View subscribers
			</Button>
		{/if}
		<Button onclick={send} disabled={publishing || !message.trim()}>
			<SendIcon />
			{publishing ? 'Publishing…' : 'Publish'}
		</Button>
	</div>
</div>
