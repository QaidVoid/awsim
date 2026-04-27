<script lang="ts">
	import { onMount } from 'svelte';
	import { listSecrets, type Secret } from '$lib/api/secrets';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import SecretDetailSheet from './secret-detail-sheet.svelte';

	let secrets = $state<Secret[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let selected = $state<Secret | null>(null);
	let detailOpen = $state(false);

	const filtered = $derived(
		filter.trim()
			? secrets.filter((s) => s.name.toLowerCase().includes(filter.trim().toLowerCase()))
			: secrets
	);

	async function load() {
		loading = true;
		try {
			secrets = await listSecrets();
		} finally {
			loading = false;
		}
	}

	function open(s: Secret) {
		selected = s;
		detailOpen = true;
	}

	onMount(load);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input
			type="search"
			placeholder="Filter secrets..."
			bind:value={filter}
			class="h-8 max-w-xs"
		/>
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {secrets.length}</Badge>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
	</div>
	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
			{loading}
			columns={[
				{ key: 'name', label: 'Name', width: '30%' },
				{ key: 'arn', label: 'ARN', mono: true },
				{ key: 'lastChangedDate', label: 'Last changed', width: '20%' }
			]}
			rowKey={(r: Secret) => r.arn || r.name}
			onRowClick={open}
		>
			{#snippet empty()}
				<EmptyState
					icon={KeyRound}
					title="No secrets stored"
					description={`Create one with: aws secretsmanager create-secret --name my/secret --secret-string '{"k":"v"}'`}
				/>
			{/snippet}
		</DataTable>
	</div>
</div>

<SecretDetailSheet
	secret={selected}
	bind:open={detailOpen}
	onOpenChange={(v) => {
		detailOpen = v;
		if (!v) selected = null;
	}}
/>
