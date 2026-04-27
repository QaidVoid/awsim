<script lang="ts">
	import { onMount } from 'svelte';
	import { listAliases, type Alias } from '$lib/api/kms';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import Tags from '@lucide/svelte/icons/tags';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';

	let aliases = $state<Alias[]>([]);
	let loading = $state(false);
	let filter = $state('');

	const filtered = $derived(
		filter.trim()
			? aliases.filter((a) =>
					a.aliasName.toLowerCase().includes(filter.trim().toLowerCase())
				)
			: aliases
	);

	async function load() {
		loading = true;
		try {
			aliases = await listAliases();
		} finally {
			loading = false;
		}
	}

	onMount(load);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input
			type="search"
			placeholder="Filter aliases..."
			bind:value={filter}
			class="h-8 max-w-xs"
		/>
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {aliases.length}</Badge>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
	</div>
	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
			columns={[
				{ key: 'aliasName', label: 'Alias', width: '35%', mono: true },
				{ key: 'targetKeyId', label: 'Target key', width: '30%', mono: true },
				{ key: 'aliasArn', label: 'ARN', mono: true }
			]}
			rowKey={(r: Alias) => r.aliasArn || r.aliasName}
		>
			{#snippet empty()}
				<EmptyState
					icon={Tags}
					title="No aliases"
					description="Aliases give a friendly name to a KMS key (e.g. alias/my-app-key)."
				/>
			{/snippet}
		</DataTable>
	</div>
</div>
