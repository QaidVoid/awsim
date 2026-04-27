<script lang="ts">
	import type { Task } from '$lib/api/ecs';
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';

	interface Props {
		task: Task | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { task, open, onOpenChange }: Props = $props();

	function shortArn(arn: string): string {
		return arn.split('/').pop() ?? arn;
	}

	function formatDate(iso?: string): string {
		if (!iso) return '—';
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full overflow-y-auto sm:max-w-xl">
		{#if task}
			<SheetHeader>
				<SheetTitle class="font-mono text-base">{shortArn(task.arn)}</SheetTitle>
				<SheetDescription>
					Task definition <span class="font-mono">{shortArn(task.taskDefinitionArn)}</span>
				</SheetDescription>
			</SheetHeader>

			<div class="flex flex-col gap-4 p-4">
				<div class="grid grid-cols-2 gap-3 rounded-md border border-border bg-card p-3 text-sm">
					<div>
						<div class="text-xs text-muted-foreground">Last status</div>
						<div class="mt-0.5"><Badge>{task.lastStatus}</Badge></div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Desired</div>
						<div class="mt-0.5"><Badge variant="outline">{task.desiredStatus}</Badge></div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Launch type</div>
						<div class="mt-0.5 font-mono text-xs">{task.launchType || '—'}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">CPU / Memory</div>
						<div class="mt-0.5 font-mono text-xs">{task.cpu || '—'} / {task.memory || '—'}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Started</div>
						<div class="mt-0.5 font-mono text-xs">{formatDate(task.startedAt)}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Stopped</div>
						<div class="mt-0.5 font-mono text-xs">{formatDate(task.stoppedAt)}</div>
					</div>
					{#if task.stoppedReason}
						<div class="col-span-2">
							<div class="text-xs text-muted-foreground">Stopped reason</div>
							<div class="mt-0.5 text-xs">{task.stoppedReason}</div>
						</div>
					{/if}
				</div>

				<section class="rounded-md border border-border bg-card">
					<header class="border-b border-border px-4 py-2">
						<h3 class="text-sm font-medium">Containers ({task.containers.length})</h3>
					</header>
					{#if task.containers.length === 0}
						<p class="px-4 py-3 text-xs text-muted-foreground">No containers reported.</p>
					{:else}
						<ul class="divide-y divide-border/40">
							{#each task.containers as c (c.name)}
								<li class="px-4 py-2">
									<div class="flex items-center justify-between gap-2">
										<span class="font-mono text-sm">{c.name}</span>
										<Badge variant="outline">{c.lastStatus || '—'}</Badge>
									</div>
									<div class="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">{c.image}</div>
									{#if c.exitCode !== undefined}
										<div class="mt-0.5 text-[11px] text-muted-foreground">exit {c.exitCode}{c.reason ? ` · ${c.reason}` : ''}</div>
									{/if}
								</li>
							{/each}
						</ul>
					{/if}
				</section>
			</div>
		{/if}
	</SheetContent>
</Sheet>
