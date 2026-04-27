<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { EmptyState, ListSkeleton } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import UsersIcon from '@lucide/svelte/icons/users';
	import { toast } from 'svelte-sonner';
	import {
		listSubscriptionsByTopic,
		subscribe,
		unsubscribe,
		type Subscription,
	} from '$lib/api/sns';

	interface Props {
		topicArn: string;
	}

	let { topicArn }: Props = $props();

	let subs = $state<Subscription[]>([]);
	let loading = $state(false);
	let creating = $state(false);
	let protocol = $state('sqs');
	let endpoint = $state('');

	const PROTOCOLS = ['sqs', 'lambda', 'http', 'https', 'email', 'sms'];

	async function load() {
		loading = true;
		try {
			subs = await listSubscriptionsByTopic(topicArn);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load subscriptions');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		// Reload when topic changes
		topicArn;
		load();
	});

	async function add() {
		if (!endpoint.trim()) {
			toast.error('Endpoint is required.');
			return;
		}
		creating = true;
		try {
			await subscribe(topicArn, protocol, endpoint.trim());
			toast.success('Subscription created.');
			endpoint = '';
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to subscribe');
		} finally {
			creating = false;
		}
	}

	async function remove(arn: string) {
		try {
			await unsubscribe(arn);
			toast.success('Unsubscribed.');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to unsubscribe');
		}
	}

	function placeholderFor(p: string): string {
		switch (p) {
			case 'sqs':
				return 'arn:aws:sqs:us-east-1:000000000000:my-queue';
			case 'lambda':
				return 'arn:aws:lambda:us-east-1:000000000000:function:my-fn';
			case 'http':
			case 'https':
				return 'https://example.com/hook';
			case 'email':
				return 'me@example.com';
			case 'sms':
				return '+15555550100';
			default:
				return '';
		}
	}
</script>

<div class="flex flex-col gap-4 p-4">
	<section class="rounded-md border border-border bg-card/40 p-4">
		<h3 class="mb-3 text-sm font-semibold">Add subscription</h3>
		<div class="grid grid-cols-1 gap-3 sm:grid-cols-[140px_1fr_auto] sm:items-end">
			<div class="flex flex-col gap-1">
				<Label for="sns-sub-protocol">Protocol</Label>
				<select
					id="sns-sub-protocol"
					bind:value={protocol}
					class="border-input dark:bg-input/30 h-9 rounded-md border bg-transparent px-2 text-sm shadow-xs outline-none focus-visible:ring-3"
				>
					{#each PROTOCOLS as p (p)}
						<option value={p}>{p}</option>
					{/each}
				</select>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="sns-sub-endpoint">Endpoint</Label>
				<Input
					id="sns-sub-endpoint"
					bind:value={endpoint}
					placeholder={placeholderFor(protocol)}
				/>
			</div>
			<Button onclick={add} disabled={creating || !endpoint.trim()}>
				{creating ? 'Subscribing…' : 'Subscribe'}
			</Button>
		</div>
	</section>

	<section>
		<div class="mb-2 flex items-center justify-between">
			<h3 class="text-sm font-semibold">Subscriptions ({subs.length})</h3>
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
		</div>

		{#if loading && subs.length === 0}
			<ListSkeleton rows={3} />
		{:else if subs.length === 0}
			<EmptyState
				icon={UsersIcon}
				title="No subscriptions"
				description="Subscribe SQS queues, Lambda functions, or webhooks to receive published messages."
			/>
		{:else}
			<ul class="flex flex-col gap-1">
				{#each subs as sub (sub.arn || sub.endpoint)}
					<li
						class="flex items-center gap-3 rounded-md border border-border bg-card/40 px-3 py-2"
					>
						<Badge variant="outline" class="h-5 px-1.5 text-[10px] uppercase">
							{sub.protocol}
						</Badge>
						<div class="min-w-0 flex-1">
							<p class="truncate font-mono text-xs">{sub.endpoint}</p>
							<p class="truncate font-mono text-[10px] text-muted-foreground">
								{sub.arn || 'PendingConfirmation'}
							</p>
						</div>
						{#if sub.arn && sub.arn !== 'PendingConfirmation'}
							<Button
								size="xs"
								variant="ghost"
								class="text-destructive hover:text-destructive"
								onclick={() => remove(sub.arn)}
							>
								<Trash2Icon />
							</Button>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</section>
</div>
