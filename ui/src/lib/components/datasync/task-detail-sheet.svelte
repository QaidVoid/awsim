<script lang="ts">
	import { describeTask, dsStatusVariant, type Task } from '$lib/api/datasync';
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { toast } from 'svelte-sonner';

	interface Props {
		task: Task | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { task, open, onOpenChange }: Props = $props();

	let detail = $state<Task | null>(null);
	let loading = $state(false);

	function fmt(ms?: number): string {
		if (!ms) return '—';
		try {
			return new Date(ms * 1000).toLocaleString();
		} catch {
			return String(ms);
		}
	}

	$effect(() => {
		if (!open || !task) {
			detail = null;
			return;
		}
		const arn = task.taskArn;
		loading = true;
		describeTask(arn)
			.then((d) => {
				detail = d ?? task;
			})
			.catch((err) => {
				toast.error(err instanceof Error ? err.message : 'Failed to load detail');
				detail = task;
			})
			.finally(() => (loading = false));
	});
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full overflow-y-auto sm:max-w-lg">
		{#if task}
			<SheetHeader>
				<SheetTitle class="font-mono text-base">{task.name ?? task.taskArn.split('/').pop()}</SheetTitle>
				<SheetDescription>
					<Badge variant={dsStatusVariant((detail ?? task).status)}>
						{(detail ?? task).status}
					</Badge>
				</SheetDescription>
			</SheetHeader>

			<div class="flex flex-col gap-4 p-4">
				<div class="rounded-md border border-border bg-card p-3">
					<div class="text-xs text-muted-foreground">Task ARN</div>
					<div class="mt-0.5 break-all font-mono text-[11px]">{task.taskArn}</div>
				</div>

				{#if loading && !detail}
					<div class="space-y-2">
						{#each Array(3) as _, i (i)}
							<Skeleton class="h-10 w-full" />
						{/each}
					</div>
				{:else if detail}
					<dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 rounded-md border border-border bg-card p-3 text-xs">
						<dt class="text-muted-foreground">Source</dt>
						<dd class="break-all font-mono">{detail.sourceLocationArn ?? '—'}</dd>
						<dt class="text-muted-foreground">Destination</dt>
						<dd class="break-all font-mono">{detail.destinationLocationArn ?? '—'}</dd>
						<dt class="text-muted-foreground">Log group</dt>
						<dd class="break-all font-mono">{detail.cloudWatchLogGroupArn ?? '—'}</dd>
						<dt class="text-muted-foreground">Created</dt>
						<dd class="font-mono">{fmt(detail.creationTime)}</dd>
						{#if detail.errorCode}
							<dt class="text-muted-foreground">Error code</dt>
							<dd class="font-mono">{detail.errorCode}</dd>
						{/if}
						{#if detail.errorDetail}
							<dt class="text-muted-foreground">Error detail</dt>
							<dd>{detail.errorDetail}</dd>
						{/if}
					</dl>
				{/if}
			</div>
		{/if}
	</SheetContent>
</Sheet>
