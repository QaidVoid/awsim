<script lang="ts">
	import {
		listTaskDefinitions,
		describeTaskDefinition,
		taskDefShortName,
		type TaskDefinition
	} from '$lib/api/ecs';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import { toast } from 'svelte-sonner';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import FileCode from '@lucide/svelte/icons/file-code';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	let arns = $state<string[]>([]);
	let loading = $state(false);
	let selected = $state<TaskDefinition | null>(null);
	let detailLoading = $state(false);

	$effect(() => {
		void load();
	});

	async function load() {
		loading = true;
		try {
			const r = await listTaskDefinitions();
			arns = r.taskDefinitionArns;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load task definitions');
		} finally {
			loading = false;
		}
	}

	async function selectDef(arn: string) {
		detailLoading = true;
		selected = null;
		try {
			selected = await describeTaskDefinition(arn);
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to describe');
		} finally {
			detailLoading = false;
		}
	}
</script>

<div class="grid h-full min-h-0 grid-cols-[280px_1fr] divide-x divide-border">
	<aside class="flex min-h-0 flex-col">
		<header class="flex items-center justify-between border-b border-border bg-background/40 px-3 py-2">
			<span class="text-xs text-muted-foreground">{arns.length} revisions</span>
			<Button type="button" variant="ghost" size="icon-sm" onclick={load} disabled={loading} aria-label="Refresh">
				<RefreshCw />
			</Button>
		</header>
		<div class="min-h-0 flex-1 overflow-y-auto">
			{#if loading && arns.length === 0}
				<div class="flex h-32 items-center justify-center text-muted-foreground">
					<Loader2 class="size-4 animate-spin" />
				</div>
			{:else if arns.length === 0}
				<div class="p-4">
					<EmptyState icon={FileCode} title="No task definitions" />
				</div>
			{:else}
				<ul class="flex flex-col">
					{#each arns as arn (arn)}
						<li>
							<button
								type="button"
								onclick={() => selectDef(arn)}
								class="block w-full truncate border-b border-border/30 px-3 py-2 text-left font-mono text-xs hover:bg-muted/50 {selected?.arn === arn ? 'bg-muted' : ''}"
							>
								{taskDefShortName(arn)}
							</button>
						</li>
					{/each}
				</ul>
			{/if}
		</div>
	</aside>

	<section class="min-h-0 overflow-y-auto p-4">
		{#if detailLoading}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if !selected}
			<EmptyState icon={FileCode} title="Select a task definition" description="Click a revision on the left." />
		{:else}
			<header class="mb-4">
				<div class="flex items-center gap-2">
					<h3 class="font-mono text-base font-semibold">{selected.family}:{selected.revision}</h3>
					<Badge variant="outline">{selected.status}</Badge>
				</div>
				<div class="mt-1 truncate font-mono text-[11px] text-muted-foreground">{selected.arn}</div>
			</header>

			<div class="grid grid-cols-3 gap-4 rounded-md border border-border bg-card p-4 text-sm">
				<div>
					<div class="text-xs text-muted-foreground">CPU</div>
					<div class="mt-0.5 font-mono">{selected.cpu || '—'}</div>
				</div>
				<div>
					<div class="text-xs text-muted-foreground">Memory</div>
					<div class="mt-0.5 font-mono">{selected.memory || '—'}</div>
				</div>
				<div>
					<div class="text-xs text-muted-foreground">Network mode</div>
					<div class="mt-0.5 font-mono">{selected.networkMode || '—'}</div>
				</div>
			</div>

			<section class="mt-4 rounded-md border border-border bg-card">
				<header class="border-b border-border px-4 py-3">
					<h4 class="text-sm font-medium">Containers ({selected.containers.length})</h4>
				</header>
				<table class="w-full text-sm">
					<thead class="text-left text-xs text-muted-foreground">
						<tr>
							<th class="px-4 py-2 font-medium">Name</th>
							<th class="px-4 py-2 font-medium">Image</th>
						</tr>
					</thead>
					<tbody>
						{#each selected.containers as c (c.name)}
							<tr class="border-t border-border/40">
								<td class="px-4 py-2 font-mono text-xs">{c.name}</td>
								<td class="px-4 py-2 font-mono text-xs text-muted-foreground">{c.image}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</section>
		{/if}
	</section>
</div>
