<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import LedgersList from '$lib/components/qldb/ledgers-list.svelte';
</script>

<ServicePage title="QLDB" description="Quantum Ledger Database. Create, inspect, and manage ledgers.">
	<LedgersList />
</ServicePage>
