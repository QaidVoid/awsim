<script lang="ts">
	import {
		getDeployments,
		createDeployment,
		deleteDeployment,
		type Deployment,
	} from '$lib/api/apigateway';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toast } from 'svelte-sonner';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		restApiId: string;
	}

	let { restApiId }: Props = $props();

	let deployments = $state<Deployment[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);

	let stageName = $state('');
	let description = $state('');
	let creating = $state(false);

	async function load() {
		loading = true;
		error = null;
		try {
			deployments = await getDeployments(restApiId);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load deployments';
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		if (restApiId) load();
	});

	async function deploy(e: Event) {
		e.preventDefault();
		if (!stageName.trim()) return;
		creating = true;
		try {
			await createDeployment(restApiId, {
				stageName: stageName.trim(),
				description: description.trim() || undefined,
			});
			toast.success(`Deployed to ${stageName.trim()}`);
			stageName = '';
			description = '';
			await load();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Deploy failed');
		} finally {
			creating = false;
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

	async function remove(d: Deployment) {
		if (!confirm(`Delete deployment ${d.id}?`)) return;
		try {
			await deleteDeployment(restApiId, d.id);
			toast.success(`Deployment ${d.id} deleted`);
			await load();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<form
		onsubmit={deploy}
		class="flex shrink-0 items-end gap-2 border-b border-border bg-background/40 px-4 py-3"
	>
		<div class="flex flex-col gap-1">
			<Label for="dep-stage">Stage</Label>
			<Input
				id="dep-stage"
				bind:value={stageName}
				placeholder="dev"
				class="h-8 w-32"
				required
			/>
		</div>
		<div class="flex flex-1 flex-col gap-1">
			<Label for="dep-desc">Description</Label>
			<Input
				id="dep-desc"
				bind:value={description}
				placeholder="optional"
				class="h-8"
			/>
		</div>
		<Button type="submit" size="sm" disabled={creating || !stageName.trim()}>
			<PlusIcon />
			{creating ? 'Deploying...' : 'Deploy'}
		</Button>
	</form>

	<div class="min-h-0 flex-1 overflow-y-auto p-4">
		{#if loading}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if error}
			<div class="text-sm text-destructive">{error}</div>
		{:else if deployments.length === 0}
			<div class="text-sm text-muted-foreground">No deployments yet.</div>
		{:else}
			<ul class="flex flex-col gap-2">
				{#each deployments as d (d.id)}
					<li class="flex items-center gap-3 rounded-md border border-border bg-card/40 p-3 text-xs">
						<code class="font-mono">{d.id}</code>
						<span class="flex-1 truncate text-muted-foreground">
							{d.description || '—'}
						</span>
						<span class="text-muted-foreground">{formatDate(d.createdDate)}</span>
						<Button
							size="sm"
							variant="ghost"
							class="h-6 gap-1 px-1.5 text-destructive"
							onclick={() => remove(d)}
							aria-label="Delete deployment"
						>
							<Trash2 class="size-3.5" />
						</Button>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>
