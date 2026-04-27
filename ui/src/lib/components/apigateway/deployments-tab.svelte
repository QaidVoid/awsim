<script lang="ts">
	import {
		getDeployments,
		createDeployment,
		type Deployment,
	} from '$lib/api/apigateway';
	import { DataTable } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toast } from 'svelte-sonner';
	import PlusIcon from '@lucide/svelte/icons/plus';

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

	const columns = [
		{ key: 'id', label: 'ID', mono: true, width: '180px' },
		{ key: 'description', label: 'Description' },
		{ key: 'createdDate', label: 'Created', width: '180px' },
	];

	let rows = $derived(
		deployments.map((d) => ({
			id: d.id,
			description: d.description || '—',
			createdDate: formatDate(d.createdDate),
		}))
	);
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

	<div class="min-h-0 flex-1">
		{#if error}
			<div class="px-4 py-4 text-sm text-destructive">{error}</div>
		{:else}
			<DataTable {rows} {columns} {loading} dense rowKey={(_r, i) => String(i)} />
		{/if}
	</div>
</div>
