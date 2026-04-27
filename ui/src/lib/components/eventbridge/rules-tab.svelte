<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import { EmptyState } from '$lib/components/service';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import FilterIcon from '@lucide/svelte/icons/filter';
	import { toast } from 'svelte-sonner';
	import { listRules, putRule, deleteRule, type Rule } from '$lib/api/eventbridge';

	interface Props {
		busName: string;
	}

	let { busName }: Props = $props();

	let rules = $state<Rule[]>([]);
	let loading = $state(false);

	let createOpen = $state(false);
	let newName = $state('');
	let newPattern = $state(JSON.stringify({ source: ['my.app'] }, null, 2));
	let newSchedule = $state('');
	let newDescription = $state('');
	let creating = $state(false);

	let confirmDelete = $state<string | null>(null);

	async function load() {
		loading = true;
		try {
			rules = await listRules(busName);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load rules');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		busName;
		load();
	});

	async function create() {
		if (!newName.trim()) {
			toast.error('Rule name is required.');
			return;
		}
		if (!newPattern.trim() && !newSchedule.trim()) {
			toast.error('Provide an event pattern or a schedule expression.');
			return;
		}
		creating = true;
		try {
			await putRule({
				name: newName.trim(),
				busName,
				eventPattern: newPattern.trim() || undefined,
				scheduleExpression: newSchedule.trim() || undefined,
				description: newDescription.trim() || undefined,
			});
			toast.success('Rule created.');
			newName = '';
			newDescription = '';
			newSchedule = '';
			createOpen = false;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create rule');
		} finally {
			creating = false;
		}
	}

	async function remove(name: string) {
		try {
			await deleteRule(name, busName);
			toast.success('Rule deleted.');
			confirmDelete = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Rules on <span class="font-mono">{busName}</span>
			<span class="ml-1 font-normal text-muted-foreground">({rules.length})</span>
		</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={() => (createOpen = true)}>
				<PlusIcon />
				New rule
			</Button>
		</div>
	</div>

	{#if rules.length === 0 && !loading}
		<EmptyState
			icon={FilterIcon}
			title="No rules"
			description="Rules match incoming events to targets via JSON event patterns or cron-like schedules."
		>
			{#snippet action()}
				<Button onclick={() => (createOpen = true)}>
					<PlusIcon />
					Create rule
				</Button>
			{/snippet}
		</EmptyState>
	{:else}
		<ul class="flex flex-col gap-2">
			{#each rules as rule (rule.arn)}
				<li class="rounded-md border border-border bg-card/40 p-3">
					<div class="flex items-start justify-between gap-3">
						<div class="min-w-0 flex-1">
							<div class="flex items-center gap-2">
								<span class="truncate font-mono text-xs font-medium">{rule.name}</span>
								<Badge
									variant="outline"
									class={rule.state === 'ENABLED'
										? 'h-4 px-1.5 text-[10px] text-green-500'
										: 'h-4 px-1.5 text-[10px] text-muted-foreground'}
								>
									{rule.state}
								</Badge>
								{#if rule.scheduleExpression}
									<Badge variant="outline" class="h-4 px-1.5 text-[10px]">Scheduled</Badge>
								{/if}
							</div>
							{#if rule.description}
								<p class="mt-1 text-[11px] text-muted-foreground">{rule.description}</p>
							{/if}
							{#if rule.scheduleExpression}
								<p class="mt-1 font-mono text-[11px] text-muted-foreground">
									{rule.scheduleExpression}
								</p>
							{/if}
							{#if rule.eventPattern}
								<pre
									class="mt-2 max-h-32 overflow-auto rounded-md border border-border bg-muted/40 p-2 text-[11px] font-mono whitespace-pre-wrap break-all">{rule.eventPattern}</pre>
							{/if}
						</div>
						<Button
							size="xs"
							variant="ghost"
							class="text-destructive hover:text-destructive"
							onclick={() => (confirmDelete = rule.name)}
						>
							<Trash2Icon />
						</Button>
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<Dialog open={createOpen} onOpenChange={(o) => (createOpen = o)}>
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle>New rule</DialogTitle>
			<DialogDescription>
				Provide an event pattern (JSON) or a schedule expression. Both can be combined.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="evb-rule-name">Name</Label>
				<Input id="evb-rule-name" bind:value={newName} placeholder="my-rule" />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="evb-rule-desc">Description</Label>
				<Input id="evb-rule-desc" bind:value={newDescription} />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="evb-rule-sched">Schedule expression (optional)</Label>
				<Input
					id="evb-rule-sched"
					bind:value={newSchedule}
					placeholder="rate(5 minutes) or cron(0 12 * * ? *)"
				/>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="evb-rule-pattern">Event pattern (JSON)</Label>
				<Textarea
					id="evb-rule-pattern"
					bind:value={newPattern}
					rows={6}
					class="font-mono text-xs"
				/>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
			<Button onclick={create} disabled={creating || !newName.trim()}>
				{creating ? 'Creating…' : 'Create rule'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>

<Dialog
	open={confirmDelete !== null}
	onOpenChange={(o) => {
		if (!o) confirmDelete = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Delete rule?</DialogTitle>
			<DialogDescription>
				Removes <span class="font-mono">{confirmDelete}</span> and any associated targets.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmDelete = null)}>Cancel</Button>
			<Button variant="destructive" onclick={() => confirmDelete && remove(confirmDelete)}>
				Delete
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
