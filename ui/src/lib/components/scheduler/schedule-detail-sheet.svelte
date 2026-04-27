<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription,
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { toast } from 'svelte-sonner';
	import { getSchedule, deleteSchedule, type Schedule } from '$lib/api/scheduler';
	import CronPreview from './cron-preview.svelte';

	interface Props {
		open: boolean;
		name: string | null;
		groupName: string;
		onOpenChange: (open: boolean) => void;
		onDeleted?: () => void;
	}

	let { open, name, groupName, onOpenChange, onDeleted }: Props = $props();

	let schedule = $state<Schedule | null>(null);
	let loading = $state(false);
	let deleting = $state(false);

	$effect(() => {
		if (open && name) {
			void load(name, groupName);
		} else if (!open) {
			schedule = null;
		}
	});

	async function load(n: string, g: string) {
		loading = true;
		try {
			schedule = await getSchedule(n, g);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load schedule');
		} finally {
			loading = false;
		}
	}

	async function handleDelete() {
		if (!schedule) return;
		deleting = true;
		try {
			await deleteSchedule(schedule.name, schedule.groupName);
			toast.success('Schedule deleted.');
			onDeleted?.();
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		} finally {
			deleting = false;
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-xl">
		<SheetHeader>
			<SheetTitle>Schedule details</SheetTitle>
			<SheetDescription>
				{#if name}
					<span class="font-mono text-xs">{name}</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4">
			{#if loading}
				<p class="text-sm text-muted-foreground">Loading…</p>
			{:else if schedule}
				<div class="flex flex-wrap items-center gap-2">
					<Badge
						variant="outline"
						class={schedule.state === 'ENABLED'
							? 'h-5 px-2 text-[10px] text-green-500'
							: 'h-5 px-2 text-[10px] text-muted-foreground'}
					>
						{schedule.state}
					</Badge>
					<Badge variant="outline" class="h-5 px-2 text-[10px]">
						Group: {schedule.groupName}
					</Badge>
					{#if schedule.scheduleExpressionTimezone}
						<Badge variant="outline" class="h-5 px-2 text-[10px]">
							TZ: {schedule.scheduleExpressionTimezone}
						</Badge>
					{/if}
				</div>

				<section>
					<h3 class="mb-1 text-xs font-semibold uppercase text-muted-foreground">
						Expression
					</h3>
					<p class="rounded-md border border-border bg-muted/40 px-3 py-2 font-mono text-xs">
						{schedule.scheduleExpression}
					</p>
					<div class="mt-2">
						<CronPreview expression={schedule.scheduleExpression} />
					</div>
				</section>

				{#if schedule.description}
					<section>
						<h3 class="mb-1 text-xs font-semibold uppercase text-muted-foreground">
							Description
						</h3>
						<p class="text-sm">{schedule.description}</p>
					</section>
				{/if}

				<section>
					<h3 class="mb-1 text-xs font-semibold uppercase text-muted-foreground">Target</h3>
					<dl class="grid grid-cols-[110px_1fr] gap-x-3 gap-y-1 text-xs">
						<dt class="text-muted-foreground">ARN</dt>
						<dd class="font-mono break-all">{schedule.target.arn}</dd>
						{#if schedule.target.roleArn}
							<dt class="text-muted-foreground">Role ARN</dt>
							<dd class="font-mono break-all">{schedule.target.roleArn}</dd>
						{/if}
						{#if schedule.target.input}
							<dt class="text-muted-foreground">Input</dt>
							<dd>
								<pre
									class="max-h-40 overflow-auto rounded-md border border-border bg-muted/40 p-2 text-[11px] font-mono whitespace-pre-wrap break-all">{schedule.target.input}</pre>
							</dd>
						{/if}
					</dl>
				</section>

				<section>
					<h3 class="mb-1 text-xs font-semibold uppercase text-muted-foreground">
						Flexible time window
					</h3>
					<p class="text-xs">
						{schedule.flexibleTimeWindow.mode}
						{#if schedule.flexibleTimeWindow.maximumWindowInMinutes}
							· up to {schedule.flexibleTimeWindow.maximumWindowInMinutes} min
						{/if}
					</p>
				</section>

				<section>
					<h3 class="mb-1 text-xs font-semibold uppercase text-muted-foreground">ARN</h3>
					<p class="font-mono text-[11px] break-all">{schedule.arn}</p>
				</section>
			{:else}
				<p class="text-sm text-muted-foreground">No schedule loaded.</p>
			{/if}
		</div>

		{#if schedule}
			<div class="flex justify-end gap-2 border-t border-border px-4 py-3">
				<Button variant="outline" onclick={() => onOpenChange(false)}>Close</Button>
				<Button variant="destructive" onclick={handleDelete} disabled={deleting}>
					<Trash2Icon />
					{deleting ? 'Deleting…' : 'Delete'}
				</Button>
			</div>
		{/if}
	</SheetContent>
</Sheet>
