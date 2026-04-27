<script lang="ts">
	import type { JobSummary } from '$lib/api/batch';
	import { jobStatusVariant, shortArn } from '$lib/api/batch';
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';

	interface Props {
		job: JobSummary | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { job, open, onOpenChange }: Props = $props();

	function fmt(ms?: number): string {
		if (!ms) return '—';
		try {
			return new Date(ms).toLocaleString();
		} catch {
			return String(ms);
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full overflow-y-auto sm:max-w-lg">
		{#if job}
			<SheetHeader>
				<SheetTitle class="font-mono text-base">{job.jobName}</SheetTitle>
				<SheetDescription>
					<Badge variant={jobStatusVariant(job.status)}>{job.status}</Badge>
				</SheetDescription>
			</SheetHeader>

			<div class="flex flex-col gap-4 p-4">
				<div class="grid grid-cols-2 gap-3 rounded-md border border-border bg-card p-3 text-sm">
					<div>
						<div class="text-xs text-muted-foreground">Job ID</div>
						<div class="mt-0.5 break-all font-mono text-[11px]">{job.jobId}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Queue</div>
						<div class="mt-0.5 font-mono text-[11px] text-muted-foreground">
							{job.jobQueue ? shortArn(job.jobQueue) : '—'}
						</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Definition</div>
						<div class="mt-0.5 font-mono text-[11px] text-muted-foreground">
							{job.jobDefinition ? shortArn(job.jobDefinition) : '—'}
						</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Created</div>
						<div class="mt-0.5 font-mono text-[11px]">{fmt(job.createdAt)}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Started</div>
						<div class="mt-0.5 font-mono text-[11px]">{fmt(job.startedAt)}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Stopped</div>
						<div class="mt-0.5 font-mono text-[11px]">{fmt(job.stoppedAt)}</div>
					</div>
					{#if job.statusReason}
						<div class="col-span-2">
							<div class="text-xs text-muted-foreground">Status reason</div>
							<div class="mt-0.5 text-xs">{job.statusReason}</div>
						</div>
					{/if}
				</div>

				{#if job.container}
					<section class="rounded-md border border-border bg-card p-3">
						<h3 class="mb-2 text-sm font-medium">Container</h3>
						<dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
							<dt class="text-muted-foreground">Image</dt>
							<dd class="font-mono">{job.container.image ?? '—'}</dd>
							{#if job.container.exitCode !== undefined}
								<dt class="text-muted-foreground">Exit code</dt>
								<dd class="font-mono">{job.container.exitCode}</dd>
							{/if}
							{#if job.container.reason}
								<dt class="text-muted-foreground">Reason</dt>
								<dd>{job.container.reason}</dd>
							{/if}
						</dl>
					</section>
				{/if}
			</div>
		{/if}
	</SheetContent>
</Sheet>
