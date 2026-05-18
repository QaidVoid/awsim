<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listJobs,
		terminateJob,
		jobStatusVariant,
		type JobSummary
	} from '$lib/api/batch';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import StopCircle from '@lucide/svelte/icons/stop-circle';
	import Briefcase from '@lucide/svelte/icons/briefcase';
	import { toast } from 'svelte-sonner';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';

	interface Props {
		onSelect: (job: JobSummary) => void;
		onSubmit: () => void;
		refreshTick?: number;
	}

	let { onSelect, onSubmit, refreshTick = 0 }: Props = $props();

	let jobs = $state<JobSummary[]>([]);
	let loading = $state(true);

	let terminateTarget = $state<JobSummary | null>(null);
	let terminateOpen = $state(false);
	let terminateBusy = $state(false);

	function fmt(ms?: number): string {
		if (!ms) return '—';
		try {
			return new Date(ms).toLocaleString();
		} catch {
			return String(ms);
		}
	}

	async function reload() {
		loading = true;
		try {
			const r = await listJobs();
			jobs = r.jobs;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load jobs');
		} finally {
			loading = false;
		}
	}

	function handleTerminate(j: JobSummary, e: Event) {
		e.stopPropagation();
		terminateTarget = j;
		terminateOpen = true;
	}

	async function confirmTerminate() {
		const j = terminateTarget;
		if (!j) return;
		terminateBusy = true;
		try {
			await terminateJob(j.jobId, 'User terminated');
			toast.success(`Terminated ${j.jobName}`);
			terminateOpen = false;
			terminateTarget = null;
			await reload();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Terminate failed');
		} finally {
			terminateBusy = false;
		}
	}

	$effect(() => {
		void refreshTick;
		reload();
	});

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{jobs.length} job{jobs.length === 1 ? '' : 's'}
		</div>
		<div class="flex items-center gap-2">
			<Button type="button" variant="outline" size="sm" onclick={reload} disabled={loading}>
				<RefreshCw class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button type="button" size="sm" onclick={onSubmit}>
				<Plus />
				Submit job
			</Button>
		</div>
	</header>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if loading && jobs.length === 0}
			<div class="space-y-2 p-4">
				{#each Array(4) as _, i (i)}
					<Skeleton class="h-7 w-full" />
				{/each}
			</div>
		{:else if jobs.length === 0}
			<div class="p-6">
				<EmptyState icon={Briefcase} title="No jobs" description="Submit a job to get started." />
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Job</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Status</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Created</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Started</th>
						<th class="px-4 py-2 text-right font-medium text-muted-foreground"></th>
					</tr>
				</thead>
				<tbody>
					{#each jobs as j (j.jobId)}
						<tr
							class="cursor-pointer border-b border-border/40 hover:bg-muted/30"
							onclick={() => onSelect(j)}
						>
							<td class="px-4 py-2">
								<div class="font-mono text-xs">{j.jobName}</div>
								<div class="font-mono text-[11px] text-muted-foreground">{j.jobId}</div>
							</td>
							<td class="px-4 py-2">
								<Badge variant={jobStatusVariant(j.status)}>{j.status}</Badge>
							</td>
							<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">{fmt(j.createdAt)}</td>
							<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">{fmt(j.startedAt)}</td>
							<td class="px-4 py-2 text-right">
								<Button
									type="button"
									variant="ghost"
									size="icon-xs"
									onclick={(e) => handleTerminate(j, e)}
									aria-label="Terminate job"
								>
									<StopCircle />
								</Button>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>

<ConfirmDialog
	bind:open={terminateOpen}
	title="Terminate job?"
	description={`Terminate job "${terminateTarget?.jobName ?? ''}".`}
	confirmLabel="Terminate"
	busy={terminateBusy}
	onConfirm={confirmTerminate}
	onClose={() => (terminateOpen = false)}
/>
