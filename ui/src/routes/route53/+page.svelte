<script lang="ts">
	import { onMount } from 'svelte';
	import { ServicePage, EmptyState, ListSkeleton } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import {
		Tabs,
		TabsList,
		TabsTrigger,
		TabsContent,
	} from '$lib/components/ui/tabs';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import InfoIcon from '@lucide/svelte/icons/info';
	import GlobeIcon from '@lucide/svelte/icons/globe';
	import { toast } from 'svelte-sonner';
	import {
		listHostedZones,
		deleteHostedZone,
		type HostedZone,
	} from '$lib/api/route53';
	import HostedZonesList from '$lib/components/route53/hosted-zones-list.svelte';
	import RecordsTab from '$lib/components/route53/records-tab.svelte';
	import HealthChecksTab from '$lib/components/route53/health-checks-tab.svelte';
	import ZoneDetailSheet from '$lib/components/route53/zone-detail-sheet.svelte';
	import CreateZoneDialog from '$lib/components/route53/create-zone-dialog.svelte';
	import CreateRecordDialog from '$lib/components/route53/create-record-dialog.svelte';

	let zones = $state<HostedZone[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	let selectedId = $state<string | null>(null);
	let activeTab = $state<'records' | 'health'>('records');
	let createZoneOpen = $state(false);
	let createRecordOpen = $state(false);
	let detailOpen = $state(false);
	let confirmDelete = $state<{ id: string; name: string } | null>(null);
	let recordsRefreshKey = $state(0);

	let selectedZone = $derived(zones.find((z) => z.id === selectedId) ?? null);

	async function loadZones() {
		loading = true;
		error = null;
		try {
			zones = await listHostedZones();
			if (selectedId && !zones.some((z) => z.id === selectedId)) selectedId = null;
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load hosted zones';
		} finally {
			loading = false;
		}
	}

	async function handleDelete() {
		if (!confirmDelete) return;
		const { id, name } = confirmDelete;
		confirmDelete = null;
		try {
			await deleteHostedZone(id);
			toast.success(`Zone ${name} deleted.`);
			detailOpen = false;
			if (selectedId === id) selectedId = null;
			await loadZones();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete zone');
		}
	}

	function bumpRecords() {
		recordsRefreshKey += 1;
	}

	onMount(loadZones);
</script>

<ServicePage
	title="Route 53"
	description="Hosted zones, DNS records, and health checks for global routing."
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={loadZones} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
		<Button size="sm" onclick={() => (createZoneOpen = true)}>
			<PlusIcon />
			New hosted zone
		</Button>
	{/snippet}

	{#if error}
		<div class="px-6 py-4 text-sm text-destructive">{error}</div>
	{:else if loading && zones.length === 0}
		<div class="px-6 py-6">
			<ListSkeleton rows={5} />
		</div>
	{:else if zones.length === 0}
		<div class="px-6 py-12">
			<EmptyState
				icon={GlobeIcon}
				title="No hosted zones"
				description="Create a hosted zone to manage DNS records for a domain."
			>
				{#snippet action()}
					<Button onclick={() => (createZoneOpen = true)}>
						<PlusIcon />
						Create hosted zone
					</Button>
				{/snippet}
			</EmptyState>
		</div>
	{:else}
		<div class="grid h-full min-h-0 grid-cols-[300px_1fr]">
			<HostedZonesList
				{zones}
				{selectedId}
				onSelect={(id) => (selectedId = id)}
				onCreate={() => (createZoneOpen = true)}
			/>

			<div class="flex h-full min-h-0 flex-col overflow-hidden">
				{#if selectedZone}
					<header
						class="flex items-center justify-between gap-3 border-b border-border px-5 py-3"
					>
						<div class="min-w-0">
							<div class="flex items-center gap-2">
								<h2 class="truncate font-mono text-sm font-medium">{selectedZone.name}</h2>
								{#if selectedZone.privateZone}
									<Badge variant="outline" class="h-4 px-1.5 text-[10px]">PRIVATE</Badge>
								{/if}
							</div>
							<p class="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">
								{selectedZone.id}
							</p>
						</div>
						<div class="flex shrink-0 items-center gap-2">
							<Button size="sm" variant="outline" onclick={() => (detailOpen = true)}>
								<InfoIcon />
								Details
							</Button>
							<Button
								size="sm"
								variant="destructive"
								onclick={() =>
									(confirmDelete = { id: selectedZone!.id, name: selectedZone!.name })}
							>
								<Trash2Icon />
								Delete
							</Button>
						</div>
					</header>

					<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
						<TabsList variant="line" class="border-b border-border px-4">
							<TabsTrigger value="records">Records</TabsTrigger>
							<TabsTrigger value="health">Health checks</TabsTrigger>
						</TabsList>

						<div class="min-h-0 flex-1 overflow-y-auto">
							<TabsContent value="records" class="m-0">
								<RecordsTab
									hostedZoneId={selectedZone.id}
									zoneName={selectedZone.name}
									onCreate={() => (createRecordOpen = true)}
									refreshKey={recordsRefreshKey}
								/>
							</TabsContent>
							<TabsContent value="health" class="m-0">
								<HealthChecksTab />
							</TabsContent>
						</div>
					</Tabs>
				{:else}
					<div class="flex h-full items-center justify-center text-sm text-muted-foreground">
						Select a hosted zone to inspect.
					</div>
				{/if}
			</div>
		</div>
	{/if}
</ServicePage>

<CreateZoneDialog
	open={createZoneOpen}
	onOpenChange={(o) => (createZoneOpen = o)}
	onCreated={loadZones}
/>

{#if selectedZone}
	<CreateRecordDialog
		open={createRecordOpen}
		onOpenChange={(o) => (createRecordOpen = o)}
		hostedZoneId={selectedZone.id}
		zoneName={selectedZone.name}
		onCreated={bumpRecords}
	/>
{/if}

<ZoneDetailSheet
	zone={selectedZone}
	open={detailOpen}
	onOpenChange={(o) => (detailOpen = o)}
	onDelete={(id) => {
		const z = zones.find((zone) => zone.id === id);
		if (z) confirmDelete = { id, name: z.name };
	}}
/>

<Dialog
	open={confirmDelete !== null}
	onOpenChange={(o) => {
		if (!o) confirmDelete = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Delete hosted zone?</DialogTitle>
			<DialogDescription>
				Permanently removes <span class="font-mono">{confirmDelete?.name}</span>. The zone
				must be empty (only NS/SOA) to delete.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmDelete = null)}>Cancel</Button>
			<Button variant="destructive" onclick={handleDelete}>Delete</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
