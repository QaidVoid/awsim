<script lang="ts">
	import { getAuthorizers, type Authorizer } from '$lib/api/apigateway';
	import { DataTable } from '$lib/components/service';

	interface Props {
		restApiId: string;
	}

	let { restApiId }: Props = $props();

	let authorizers = $state<Authorizer[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);

	async function load() {
		loading = true;
		error = null;
		try {
			authorizers = await getAuthorizers(restApiId);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load authorizers';
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		if (restApiId) load();
	});

	const columns = [
		{ key: 'name', label: 'Name', width: '180px' },
		{ key: 'type', label: 'Type', width: '120px' },
		{ key: 'authType', label: 'Auth type', width: '120px' },
		{ key: 'identitySource', label: 'Identity source', mono: true },
	];

	let rows = $derived(
		authorizers.map((a) => ({
			name: a.name || a.id,
			type: a.type || '—',
			authType: a.authType || '—',
			identitySource: a.identitySource || '—',
		}))
	);
</script>

<div class="h-full min-h-0">
	{#if error}
		<div class="px-4 py-4 text-sm text-destructive">{error}</div>
	{:else}
		<DataTable {rows} {columns} {loading} dense rowKey={(_r, i) => String(i)} />
	{/if}
</div>
