<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { toast } from 'svelte-sonner';
	import {
		listTargetsByRule,
		putTargets,
		removeTargets,
		type Target,
	} from '$lib/api/eventbridge';

	interface Props {
		ruleName: string;
		busName: string;
	}

	let { ruleName, busName }: Props = $props();

	// AWS caps a rule at five targets and rejects the whole call past
	// that, so the add button goes away rather than failing on submit.
	const MAX_TARGETS = 5;

	let targets = $state<Target[]>([]);
	let loading = $state(false);
	let addOpen = $state(false);
	let saving = $state(false);
	let pendingRemove = $state<string | null>(null);

	let id = $state('');
	let arn = $state('');
	let input = $state('');
	let inputPath = $state('');
	let roleArn = $state('');
	let deadLetterArn = $state('');
	let maxAge = $state<number | null>(null);
	let maxRetries = $state<number | null>(null);

	$effect(() => {
		ruleName;
		busName;
		void load();
	});

	async function load() {
		loading = true;
		try {
			targets = await listTargetsByRule(ruleName, busName);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load targets');
			targets = [];
		} finally {
			loading = false;
		}
	}

	function reset() {
		id = '';
		arn = '';
		input = '';
		inputPath = '';
		roleArn = '';
		deadLetterArn = '';
		maxAge = null;
		maxRetries = null;
	}

	async function add() {
		if (!id.trim()) {
			toast.error('Target id is required.');
			return;
		}
		if (!arn.trim().startsWith('arn:')) {
			toast.error('Target ARN must start with `arn:`.');
			return;
		}
		if (input.trim() && inputPath.trim()) {
			toast.error('Input and InputPath are mutually exclusive.');
			return;
		}
		if (input.trim()) {
			try {
				JSON.parse(input);
			} catch {
				toast.error('Input must be valid JSON.');
				return;
			}
		}
		saving = true;
		try {
			const res = await putTargets(
				ruleName,
				[
					{
						id: id.trim(),
						arn: arn.trim(),
						input: input.trim() || undefined,
						inputPath: inputPath.trim() || undefined,
						roleArn: roleArn.trim() || undefined,
						deadLetterArn: deadLetterArn.trim() || undefined,
						maximumEventAgeInSeconds: maxAge ?? undefined,
						maximumRetryAttempts: maxRetries ?? undefined,
					},
				],
				busName,
			);
			// PutTargets answers 200 even when a target was rejected, so a
			// blind success toast would report the opposite of what happened.
			if (res.failedEntryCount > 0) {
				toast.error(res.messages.join('; ') || 'Target was rejected.');
				return;
			}
			toast.success('Target added.');
			reset();
			addOpen = false;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to add target');
		} finally {
			saving = false;
		}
	}

	async function remove(targetId: string) {
		try {
			const res = await removeTargets(ruleName, [targetId], busName);
			if (res.failedEntryCount > 0) {
				toast.error(res.messages.join('; ') || 'Target was not removed.');
				return;
			}
			toast.success('Target removed.');
			pendingRemove = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to remove target');
		}
	}

	/** `arn:aws:sqs:us-east-1:0:q` reads as "sqs", which is what a reader scans for. */
	function serviceOf(a: string): string {
		return a.split(':')[2] ?? 'target';
	}
</script>

<div class="mt-3 rounded-md border border-border bg-background/40 p-2">
	<div class="flex items-center justify-between">
		<span class="text-[11px] font-medium text-muted-foreground">
			Targets ({targets.length}/{MAX_TARGETS})
		</span>
		{#if targets.length < MAX_TARGETS}
			<Button size="xs" variant="ghost" onclick={() => (addOpen = true)}>
				<PlusIcon />
				Add target
			</Button>
		{/if}
	</div>

	{#if loading}
		<p class="px-1 py-2 text-[11px] text-muted-foreground">Loading targets...</p>
	{:else if targets.length === 0}
		<p class="px-1 py-2 text-[11px] text-muted-foreground">
			No targets. A rule with no targets matches events and then discards them.
		</p>
	{:else}
		<ul class="mt-1 flex flex-col gap-1">
			{#each targets as t (t.id)}
				<li class="flex items-start justify-between gap-2 rounded border border-border/60 px-2 py-1.5">
					<div class="min-w-0 flex-1">
						<div class="flex flex-wrap items-center gap-1.5">
							<span class="font-mono text-[11px] font-medium">{t.id}</span>
							<Badge variant="outline" class="h-4 px-1.5 text-[10px]">
								{serviceOf(t.arn)}
							</Badge>
							{#if t.deadLetterArn}
								<Badge variant="outline" class="h-4 px-1.5 text-[10px]">DLQ</Badge>
							{/if}
							{#if t.maximumRetryAttempts !== undefined}
								<Badge variant="outline" class="h-4 px-1.5 text-[10px]">
									{t.maximumRetryAttempts} retries
								</Badge>
							{/if}
						</div>
						<p class="truncate font-mono text-[10px] text-muted-foreground">{t.arn}</p>
						{#if t.input}
							<pre
								class="mt-1 max-h-20 overflow-auto rounded bg-muted/40 p-1 text-[10px] font-mono whitespace-pre-wrap break-all">{t.input}</pre>
						{/if}
						{#if t.inputPath}
							<p class="font-mono text-[10px] text-muted-foreground">InputPath: {t.inputPath}</p>
						{/if}
					</div>
					<Button
						size="xs"
						variant="ghost"
						class="text-destructive hover:text-destructive"
						onclick={() => (pendingRemove = t.id)}
						aria-label="Remove target"
					>
						<Trash2Icon />
					</Button>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<Dialog open={addOpen} onOpenChange={(o) => (addOpen = o)}>
	<DialogContent class="sm:max-w-lg max-h-[85vh] overflow-y-auto">
		<DialogHeader>
			<DialogTitle>Add target</DialogTitle>
			<DialogDescription>
				What <span class="font-mono">{ruleName}</span> invokes when an event matches.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="evb-target-id">Target id</Label>
				<Input id="evb-target-id" bind:value={id} placeholder="order-queue" />
				<p class="text-[11px] text-muted-foreground">
					Unique within the rule. Reusing an id replaces that target.
				</p>
			</div>

			<div class="flex flex-col gap-1">
				<Label for="evb-target-arn">Target ARN</Label>
				<Input
					id="evb-target-arn"
					bind:value={arn}
					placeholder="arn:aws:sqs:us-east-1:000000000000:orders"
					class="font-mono text-xs"
				/>
			</div>

			<div class="flex flex-col gap-1">
				<Label for="evb-target-input">Input (optional)</Label>
				<Textarea
					id="evb-target-input"
					bind:value={input}
					rows={3}
					placeholder={'{"key": "value"}'}
					class="font-mono text-xs"
				/>
				<p class="text-[11px] text-muted-foreground">
					Constant JSON delivered instead of the event itself.
				</p>
			</div>

			<div class="flex flex-col gap-1">
				<Label for="evb-target-inputpath">InputPath (optional)</Label>
				<Input
					id="evb-target-inputpath"
					bind:value={inputPath}
					placeholder="$.detail"
					class="font-mono text-xs"
				/>
				<p class="text-[11px] text-muted-foreground">
					A JSONPath slice of the event. Cannot be combined with Input.
				</p>
			</div>

			<div class="flex flex-col gap-1">
				<Label for="evb-target-role">Role ARN (optional)</Label>
				<Input id="evb-target-role" bind:value={roleArn} class="font-mono text-xs" />
			</div>

			<div class="flex flex-col gap-1">
				<Label for="evb-target-dlq">Dead letter queue ARN (optional)</Label>
				<Input
					id="evb-target-dlq"
					bind:value={deadLetterArn}
					placeholder="arn:aws:sqs:us-east-1:000000000000:dlq"
					class="font-mono text-xs"
				/>
				<p class="text-[11px] text-muted-foreground">
					Where events go once retries are exhausted. Without one they are dropped.
				</p>
			</div>

			<div class="grid grid-cols-2 gap-3">
				<div class="flex flex-col gap-1">
					<Label for="evb-target-age">Max event age (s)</Label>
					<Input
						id="evb-target-age"
						type="number"
						min="60"
						max="86400"
						bind:value={maxAge}
						placeholder="86400"
					/>
				</div>
				<div class="flex flex-col gap-1">
					<Label for="evb-target-retries">Max retries</Label>
					<Input
						id="evb-target-retries"
						type="number"
						min="0"
						max="185"
						bind:value={maxRetries}
						placeholder="185"
					/>
				</div>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => (addOpen = false)}>Cancel</Button>
			<Button onclick={add} disabled={saving || !id.trim() || !arn.trim()}>
				{saving ? 'Adding...' : 'Add target'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>

<Dialog
	open={pendingRemove !== null}
	onOpenChange={(o) => {
		if (!o) pendingRemove = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Remove target?</DialogTitle>
			<DialogDescription>
				<span class="font-mono">{pendingRemove}</span> stops receiving events from
				<span class="font-mono">{ruleName}</span>.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (pendingRemove = null)}>Cancel</Button>
			<Button variant="destructive" onclick={() => pendingRemove && remove(pendingRemove)}>
				Remove
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
