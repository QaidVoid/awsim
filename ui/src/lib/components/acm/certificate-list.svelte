<script lang="ts">
	import { onMount } from 'svelte';
	import { listCertificates, type Certificate } from '$lib/api/acm';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import CertificateDetailSheet from './certificate-detail-sheet.svelte';
	import RequestDialog from './request-dialog.svelte';

	let certs = $state<Certificate[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let selected = $state<Certificate | null>(null);
	let detailOpen = $state(false);
	let requestOpen = $state(false);

	const filtered = $derived(
		filter.trim()
			? certs.filter((c) => c.domainName.toLowerCase().includes(filter.trim().toLowerCase()))
			: certs
	);

	async function load() {
		loading = true;
		try {
			certs = await listCertificates();
		} finally {
			loading = false;
		}
	}

	function openCert(c: Certificate) {
		selected = c;
		detailOpen = true;
	}

	function statusVariant(s: string): 'secondary' | 'destructive' | 'outline' {
		if (s === 'ISSUED') return 'secondary';
		if (s === 'EXPIRED' || s === 'FAILED' || s === 'REVOKED' || s === 'VALIDATION_TIMED_OUT')
			return 'destructive';
		return 'outline';
	}

	onMount(load);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input
			type="search"
			placeholder="Filter certificates..."
			bind:value={filter}
			class="h-8 max-w-xs"
		/>
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {certs.length}</Badge>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
		<Button size="sm" onclick={() => (requestOpen = true)}>
			<Plus class="size-3.5" /> Request
		</Button>
	</div>
	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
			{loading}
			columns={[
				{ key: 'domainName', label: 'Domain', width: '30%' },
				{ key: 'arn', label: 'ARN', mono: true },
				{ key: 'status', label: 'Status', width: '20%', cell: cellStatus }
			]}
			rowKey={(r: Certificate) => r.arn}
			onRowClick={openCert}
		>
			{#snippet empty()}
				<EmptyState
					icon={ShieldCheck}
					title="No certificates"
					description="Request one to get started — domain validation happens via DNS or email."
				/>
			{/snippet}
		</DataTable>
	</div>
</div>

{#snippet cellStatus(r: Certificate)}
	<Badge variant={statusVariant(r.status)}>{r.status}</Badge>
{/snippet}

<CertificateDetailSheet
	cert={selected}
	bind:open={detailOpen}
	onOpenChange={(v) => {
		detailOpen = v;
		if (!v) selected = null;
	}}
/>

<RequestDialog
	bind:open={requestOpen}
	onOpenChange={(v) => (requestOpen = v)}
	onCreated={load}
/>
