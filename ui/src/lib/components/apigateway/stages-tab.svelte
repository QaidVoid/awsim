<script lang="ts">
	import { getStages, deleteStage, stageInvokeUrl, type Stage } from '$lib/api/apigateway';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { toast } from 'svelte-sonner';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Copy from '@lucide/svelte/icons/copy';
	import Trash2 from '@lucide/svelte/icons/trash-2';

	interface Props {
		restApiId: string;
	}

	let { restApiId }: Props = $props();

	let stages = $state<Stage[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);

	async function load() {
		loading = true;
		error = null;
		try {
			stages = await getStages(restApiId);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load stages';
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		if (restApiId) load();
	});

	async function copyUrl(stage: string) {
		const url = stageInvokeUrl(restApiId, stage);
		try {
			await navigator.clipboard.writeText(url);
			toast.success('Invoke URL copied');
		} catch {
			toast.error('Copy failed');
		}
	}

	async function removeStage(stage: Stage) {
		if (!confirm(`Delete stage ${stage.stageName}?`)) return;
		try {
			await deleteStage(restApiId, stage.stageName);
			toast.success(`Stage ${stage.stageName} deleted`);
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		}
	}

	function formatDate(iso: string): string {
		if (!iso) return '—';
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}
</script>

<div class="p-4">
	{#if loading}
		<div class="flex h-32 items-center justify-center text-muted-foreground">
			<Loader2 class="size-4 animate-spin" />
		</div>
	{:else if error}
		<div class="text-sm text-destructive">{error}</div>
	{:else if stages.length === 0}
		<div class="text-sm text-muted-foreground">No stages deployed yet.</div>
	{:else}
		<ul class="flex flex-col gap-2">
			{#each stages as stage (stage.stageName)}
				<li class="rounded-md border border-border bg-card/40 p-3">
					<div class="mb-2 flex items-center gap-2">
						<span class="font-mono text-sm font-medium">{stage.stageName}</span>
						{#if stage.cacheClusterEnabled}
							<Badge variant="outline" class="h-4 px-1 text-[10px]">cache</Badge>
						{/if}
						<Button
							size="sm"
							variant="ghost"
							class="ml-auto h-6 gap-1 px-1.5 text-destructive"
							onclick={() => removeStage(stage)}
							aria-label="Delete stage"
						>
							<Trash2 class="size-3.5" />
						</Button>
					</div>
					<div class="grid grid-cols-[110px_1fr] gap-x-2 gap-y-0.5 text-xs">
						<span class="text-muted-foreground">Deployment</span>
						<span class="font-mono">{stage.deploymentId || '—'}</span>
						<span class="text-muted-foreground">Created</span>
						<span>{formatDate(stage.createdDate)}</span>
						<span class="text-muted-foreground">Updated</span>
						<span>{formatDate(stage.lastUpdatedDate)}</span>
						<span class="text-muted-foreground">Invoke URL</span>
						<span class="flex items-center gap-1">
							<code class="truncate font-mono text-[11px]">
								{stageInvokeUrl(restApiId, stage.stageName)}
							</code>
							<Button
								size="sm"
								variant="ghost"
								class="h-6 w-6 p-0"
								onclick={() => copyUrl(stage.stageName)}
								aria-label="Copy invoke URL"
							>
								<Copy class="size-3" />
							</Button>
						</span>
						{#if Object.keys(stage.variables).length > 0}
							<span class="text-muted-foreground">Variables</span>
							<div class="flex flex-wrap gap-1">
								{#each Object.entries(stage.variables) as [k, v] (k)}
									<Badge variant="outline" class="h-4 px-1 text-[10px]">
										{k}={v}
									</Badge>
								{/each}
							</div>
						{/if}
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</div>
