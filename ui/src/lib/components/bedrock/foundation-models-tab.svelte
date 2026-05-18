<script lang="ts">
	import { onMount } from 'svelte';
	import { listFoundationModels, type FoundationModel } from '$lib/api/bedrock';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import SearchIcon from '@lucide/svelte/icons/search';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import { toast } from 'svelte-sonner';

	interface Props {
		onSelect: (model: FoundationModel) => void;
	}

	let { onSelect }: Props = $props();

	let models = $state<FoundationModel[]>([]);
	let loading = $state(true);
	let filter = $state('');
	let provider = $state<string>('all');

	let providers = $derived(
		Array.from(new Set(models.map((m) => m.providerName))).filter(Boolean).sort()
	);
	let filtered = $derived(
		models.filter(
			(m) =>
				(provider === 'all' || m.providerName === provider) &&
				(filter.trim() === '' ||
					m.modelId.toLowerCase().includes(filter.trim().toLowerCase()) ||
					m.modelName.toLowerCase().includes(filter.trim().toLowerCase()))
		)
	);

	onMount(load);

	async function load() {
		loading = true;
		try {
			models = await listFoundationModels();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load models');
		} finally {
			loading = false;
		}
	}

	function modalityVariant(m: string): 'secondary' | 'outline' {
		if (m === 'TEXT' || m === 'EMBEDDING') return 'secondary';
		return 'outline';
	}

	let providerLabel = $derived(provider === 'all' ? 'All providers' : provider);
</script>

{#snippet providerCell(row: FoundationModel)}
	<span class="text-xs">{row.providerName}</span>
{/snippet}

{#snippet modalitiesCell(row: FoundationModel)}
	<div class="flex flex-wrap gap-1">
		{#each row.inputModalities as m (m)}
			<Badge variant={modalityVariant(m)} class="h-4 px-1 text-[10px]">{m}</Badge>
		{/each}
	</div>
{/snippet}

{#snippet streamingCell(row: FoundationModel)}
	{#if row.responseStreamingSupported}
		<Badge variant="secondary" class="h-4 px-1 text-[10px]">stream</Badge>
	{:else}
		<span class="text-[10px] text-muted-foreground">—</span>
	{/if}
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center gap-2">
		<div class="relative flex-1">
			<SearchIcon
				class="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground"
			/>
			<Input
				type="search"
				placeholder="Filter by name or ID"
				bind:value={filter}
				class="h-8 pl-7 text-xs"
			/>
		</div>
		<Select type="single" bind:value={provider}>
			<SelectTrigger aria-label="Filter by provider" size="sm" class="w-[160px] text-xs">
				{providerLabel}
			</SelectTrigger>
			<SelectContent>
				<SelectItem value="all" label="All providers">All providers</SelectItem>
				{#each providers as p (p)}
					<SelectItem value={p} label={p}>{p}</SelectItem>
				{/each}
			</SelectContent>
		</Select>
		<Button variant="outline" size="sm" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		rows={filtered}
		{loading}
		columns={[
			{ key: 'modelId', label: 'Model ID', mono: true },
			{ key: 'modelName', label: 'Name' },
			{ key: 'providerName', label: 'Provider', cell: providerCell },
			{ key: 'modalities', label: 'Input', cell: modalitiesCell },
			{ key: 'streaming', label: 'Streaming', cell: streamingCell },
		]}
		rowKey={(r) => r.modelArn || r.modelId}
		onRowClick={onSelect}
	>
		{#snippet empty()}
			<EmptyState
				icon={SparklesIcon}
				title="No foundation models"
				description="No models match the current filter."
			/>
		{/snippet}
	</DataTable>
</div>
