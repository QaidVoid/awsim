<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import ServersList from '$lib/components/transfer/servers-list.svelte';
	import UsersSheet from '$lib/components/transfer/users-sheet.svelte';
	import type { ServerSummary } from '$lib/api/transfer';

	let detailOpen = $state(false);
	let detailServer = $state<ServerSummary | null>(null);
	let refreshKey = $state(0);

	function refresh() {
		refreshKey += 1;
	}

	function openDetail(s: ServerSummary) {
		detailServer = s;
		detailOpen = true;
	}
</script>

<ServicePage
	title="Transfer Family"
	description="SFTP / FTPS / FTP servers, users, and SSH keys."
>
	<ServersList onSelect={openDetail} {refreshKey} />
</ServicePage>

<UsersSheet
	open={detailOpen}
	server={detailServer}
	onOpenChange={(o) => (detailOpen = o)}
	onChanged={refresh}
/>
