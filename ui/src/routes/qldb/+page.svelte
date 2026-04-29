<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import LedgersList from '$lib/components/qldb/ledgers-list.svelte';
</script>

<ServicePage title="QLDB" description="Quantum Ledger Database — ledger metadata only.">
	<LedgersList />
</ServicePage>
