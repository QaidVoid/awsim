<script lang="ts">
	import { listFargateProfilesWithDetail, type Cluster, type FargateProfile } from '$lib/api/eks';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import { toast } from 'svelte-sonner';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Layers from '@lucide/svelte/icons/layers';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		cluster: Cluster | null;
	}

	let { cluster }: Props = $props();

	let profiles = $state<FargateProfile[]>([]);
	let loading = $state(false);
	let lastName = $state('');

	$effect(() => {
		if (cluster && cluster.name !== lastName) {
			lastName = cluster.name;
			void load();
		}
		if (!cluster) {
			profiles = [];
			lastName = '';
		}
	});

	async function load() {
		if (!cluster) return;
		loading = true;
		try {
			const r = await listFargateProfilesWithDetail(cluster.name);
			profiles = r.profiles;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load Fargate profiles');
		} finally {
			loading = false;
		}
	}

	function statusVariant(s: string): 'default' | 'secondary' | 'destructive' | 'outline' {
		if (s === 'ACTIVE') return 'default';
		if (s === 'CREATE_FAILED' || s === 'DELETE_FAILED') return 'destructive';
		return 'secondary';
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{cluster ? `Fargate profiles in ${cluster.name}` : 'Pick a cluster'}
		</div>
		<Button type="button" variant="outline" size="sm" onclick={load} disabled={loading || !cluster}>
			<RefreshCw />
			Refresh
		</Button>
	</header>

	<div class="min-h-0 flex-1 overflow-y-auto">
		{#if !cluster}
			<div class="p-6">
				<EmptyState icon={Layers} title="No cluster selected" description="Choose a cluster from the Clusters tab." />
			</div>
		{:else if loading && profiles.length === 0}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if profiles.length === 0}
			<div class="p-6">
				<EmptyState icon={Layers} title="No Fargate profiles" description="This cluster has no Fargate profiles." />
			</div>
		{:else}
			<ul class="flex flex-col divide-y divide-border/40">
				{#each profiles as p (p.name)}
					<li class="flex flex-col gap-2 px-4 py-3">
						<div class="flex items-center justify-between gap-2">
							<div class="flex items-center gap-2">
								<span class="font-mono text-sm">{p.name}</span>
								<Badge variant={statusVariant(p.status)}>{p.status}</Badge>
							</div>
							<span class="truncate font-mono text-[11px] text-muted-foreground">{p.arn}</span>
						</div>
						<div class="grid grid-cols-2 gap-3 text-xs text-muted-foreground">
							<div>
								<span class="text-foreground/70">Pod execution role:</span>
								<span class="ml-1 font-mono">{p.podExecutionRoleArn || '—'}</span>
							</div>
							<div>
								<span class="text-foreground/70">Selectors:</span>
								<span class="ml-1 font-mono">
									{p.selectors.length > 0
										? p.selectors.map((s) => s.namespace).join(', ')
										: '—'}
								</span>
							</div>
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>
