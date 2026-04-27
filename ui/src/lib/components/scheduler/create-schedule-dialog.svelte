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
	import { createSchedule, type ScheduleGroup } from '$lib/api/scheduler';
	import CronPreview from './cron-preview.svelte';

	interface Props {
		open: boolean;
		groups: ScheduleGroup[];
		defaultGroup?: string;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let { open, groups, defaultGroup = 'default', onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let groupName = $state('default');
	let expression = $state('rate(1 hour)');
	let timezone = $state('');
	let description = $state('');
	let targetArn = $state('');
	let targetRoleArn = $state('');
	let targetInput = $state('');
	let creating = $state(false);

	$effect(() => {
		if (open) {
			groupName = defaultGroup;
		}
	});

	async function submit() {
		if (!name.trim()) {
			toast.error('Schedule name is required.');
			return;
		}
		if (!expression.trim()) {
			toast.error('Schedule expression is required.');
			return;
		}
		if (!targetArn.trim()) {
			toast.error('Target ARN is required.');
			return;
		}
		creating = true;
		try {
			await createSchedule({
				name: name.trim(),
				groupName: groupName || 'default',
				scheduleExpression: expression.trim(),
				scheduleExpressionTimezone: timezone.trim() || undefined,
				description: description.trim() || undefined,
				target: {
					arn: targetArn.trim(),
					roleArn: targetRoleArn.trim() || undefined,
					input: targetInput.trim() || undefined,
				},
			});
			toast.success(`Schedule ${name.trim()} created.`);
			name = '';
			targetArn = '';
			targetRoleArn = '';
			targetInput = '';
			description = '';
			onCreated?.();
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create schedule');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-xl">
		<DialogHeader>
			<DialogTitle>New schedule</DialogTitle>
			<DialogDescription>
				Create an EventBridge Scheduler entry that fires at a fixed rate or cron expression.
			</DialogDescription>
		</DialogHeader>

		<div class="flex max-h-[70vh] flex-col gap-3 overflow-y-auto px-4">
			<div class="grid grid-cols-2 gap-3">
				<div class="flex flex-col gap-1">
					<Label for="sched-name">Name</Label>
					<Input id="sched-name" bind:value={name} placeholder="my-schedule" />
				</div>
				<div class="flex flex-col gap-1">
					<Label for="sched-group">Group</Label>
					<Input
						id="sched-group"
						bind:value={groupName}
						list="sched-groups"
						placeholder="default"
					/>
					<datalist id="sched-groups">
						{#each groups as g (g.arn)}
							<option value={g.name}></option>
						{/each}
					</datalist>
				</div>
			</div>

			<div class="flex flex-col gap-1">
				<Label for="sched-expr">Schedule expression</Label>
				<Input
					id="sched-expr"
					bind:value={expression}
					placeholder="rate(1 hour) or cron(0 12 * * ? *)"
				/>
				<CronPreview {expression} />
			</div>

			<div class="grid grid-cols-2 gap-3">
				<div class="flex flex-col gap-1">
					<Label for="sched-tz">Timezone (optional)</Label>
					<Input id="sched-tz" bind:value={timezone} placeholder="UTC" />
				</div>
				<div class="flex flex-col gap-1">
					<Label for="sched-desc">Description</Label>
					<Input id="sched-desc" bind:value={description} />
				</div>
			</div>

			<div class="flex flex-col gap-1">
				<Label for="sched-target">Target ARN</Label>
				<Input
					id="sched-target"
					bind:value={targetArn}
					placeholder="arn:aws:lambda:us-east-1:000000000000:function:my-fn"
					class="font-mono text-xs"
				/>
			</div>

			<div class="flex flex-col gap-1">
				<Label for="sched-role">Target role ARN (optional)</Label>
				<Input
					id="sched-role"
					bind:value={targetRoleArn}
					placeholder="arn:aws:iam::000000000000:role/scheduler-role"
					class="font-mono text-xs"
				/>
			</div>

			<div class="flex flex-col gap-1">
				<Label for="sched-input">Target input (JSON, optional)</Label>
				<Textarea
					id="sched-input"
					bind:value={targetInput}
					rows={4}
					placeholder={'{"key":"value"}'}
					class="font-mono text-xs"
				/>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button
				onclick={submit}
				disabled={creating || !name.trim() || !expression.trim() || !targetArn.trim()}
			>
				{creating ? 'Creating…' : 'Create schedule'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
