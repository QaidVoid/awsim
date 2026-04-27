<script lang="ts">
	import { onMount } from 'svelte';
	import { listTypes, type ApiType } from '$lib/api/appsync';
	import { EmptyState, ListSkeleton } from '$lib/components/service';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import { toast } from 'svelte-sonner';

	interface Props {
		apiId: string;
	}

	let { apiId }: Props = $props();

	let types = $state<ApiType[]>([]);
	let loading = $state(true);

	let lastApiId = $state<string | null>(null);
	$effect(() => {
		if (apiId !== lastApiId) {
			lastApiId = apiId;
			load();
		}
	});

	onMount(load);

	async function load() {
		loading = true;
		try {
			types = await listTypes(apiId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load schema');
		} finally {
			loading = false;
		}
	}
</script>

<div class="p-4">
	{#if loading}
		<ListSkeleton rows={5} />
	{:else if types.length === 0}
		<EmptyState
			icon={FileTextIcon}
			title="No types"
			description="Define a schema to expose Query, Mutation, and custom types."
		/>
	{:else}
		<div class="flex flex-col gap-3">
			{#each types as t (t.name)}
				<div class="rounded-md border border-border bg-card/40 p-3">
					<div class="mb-2 flex items-center justify-between">
						<span class="font-mono text-sm font-medium">{t.name}</span>
						<span class="text-[10px] uppercase tracking-wide text-muted-foreground">
							{t.format}
						</span>
					</div>
					{#if t.definition}
						<pre
							class="max-h-64 overflow-auto rounded bg-muted/40 p-2 font-mono text-[11px] leading-snug">{t.definition}</pre>
					{:else}
						<p class="text-xs text-muted-foreground">Empty type.</p>
					{/if}
				</div>
			{/each}
		</div>
	{/if}
</div>
