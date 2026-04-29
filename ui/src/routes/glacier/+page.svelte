<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import VaultsList from '$lib/components/glacier/vaults-list.svelte';
	import VaultDetailSheet from '$lib/components/glacier/vault-detail-sheet.svelte';
	import type { Vault } from '$lib/api/glacier';

	let detailOpen = $state(false);
	let detailVault = $state<Vault | null>(null);
	let refreshKey = $state(0);

	function refresh() {
		refreshKey += 1;
	}

	function openDetail(v: Vault) {
		detailVault = v;
		detailOpen = true;
	}
</script>

<ServicePage title="Glacier" description="Cold-storage vaults and archives.">
	<VaultsList onSelect={openDetail} {refreshKey} onChanged={refresh} />
</ServicePage>

<VaultDetailSheet
	open={detailOpen}
	vault={detailVault}
	onOpenChange={(o) => (detailOpen = o)}
	onChanged={refresh}
/>
