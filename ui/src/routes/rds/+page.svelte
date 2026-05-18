<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		describeDBInstances,
		deleteDBInstance,
		type DBInstance
	} from '$lib/api/rds';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import InstanceList from '$lib/components/rds/instance-list.svelte';
	import InstanceDetailSheet from '$lib/components/rds/instance-detail-sheet.svelte';
	import SnapshotsTab from '$lib/components/rds/snapshots-tab.svelte';
	import CreateInstanceDialog from '$lib/components/rds/create-instance-dialog.svelte';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import Plus from '@lucide/svelte/icons/plus';

	let instances = $state<DBInstance[]>([]);
	let loading = $state(true);

	let active: string = $state(
		useTab('rds', ['instances', 'snapshots'] as const, 'instances', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);

	let selected = $state<DBInstance | null>(null);
	let detailOpen = $state(false);

	let createOpen = $state(false);
	let confirmOpen = $state(false);
	let confirmBusy = $state(false);
	let pendingDelete = $state<DBInstance | null>(null);

	onMount(refresh);

	async function refresh() {
		loading = true;
		try {
			instances = await describeDBInstances();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to list DB instances');
		} finally {
			loading = false;
		}
	}

	function openInstance(inst: DBInstance) {
		selected = inst;
		detailOpen = true;
	}

	function askDelete(inst: DBInstance) {
		pendingDelete = inst;
		confirmOpen = true;
	}

	async function confirmDelete() {
		if (!pendingDelete) return;
		confirmBusy = true;
		try {
			await deleteDBInstance(pendingDelete.identifier);
			toast.success(`Delete requested for ${pendingDelete.identifier}`);
			confirmOpen = false;
			detailOpen = false;
			pendingDelete = null;
			await refresh();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete instance');
		} finally {
			confirmBusy = false;
		}
	}

	async function onCreated() {
		await refresh();
	}
</script>

<ServicePage title="RDS" description="Relational Database Service — instances and snapshots.">
	{#snippet actions()}
		<Badge variant="outline" class="font-mono">
			{instances.length} instance{instances.length === 1 ? '' : 's'}
		</Badge>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<Plus class="size-3.5" />
			Create instance
		</Button>
	{/snippet}

	<Tabs bind:value={active} class="flex h-full min-h-0 flex-col gap-0">
		<TabsList class="mx-4 mt-2 self-start">
			<TabsTrigger value="instances">Instances</TabsTrigger>
			<TabsTrigger value="snapshots">Snapshots</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1">
			<TabsContent value="instances" class="m-0 h-full">
				<InstanceList
					{instances}
					{loading}
					selectedId={selected?.identifier ?? null}
					onSelect={openInstance}
					onRefresh={refresh}
				/>
			</TabsContent>
			<TabsContent value="snapshots" class="m-0 h-full">
				<SnapshotsTab />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<InstanceDetailSheet
	bind:open={detailOpen}
	instance={selected}
	onClose={() => (detailOpen = false)}
	onDeleteInstance={askDelete}
/>

<CreateInstanceDialog
	bind:open={createOpen}
	onClose={() => (createOpen = false)}
	onCreated={onCreated}
/>

<ConfirmDialog
	bind:open={confirmOpen}
	title="Delete DB instance?"
	description={`Permanently delete "${pendingDelete?.identifier ?? ''}". The final snapshot will be skipped.`}
	busy={confirmBusy}
	onConfirm={confirmDelete}
	onClose={() => (confirmOpen = false)}
/>
