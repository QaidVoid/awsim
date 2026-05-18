<script lang="ts">
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import {
		createGlobalTable,
		listGlobalTables,
		updateGlobalTable,
		type GlobalTable
	} from '$lib/api/dynamodb';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { toast } from 'svelte-sonner';

	interface Props {
		open: boolean;
		onClose: () => void;
	}
	let { open = $bindable(false), onClose }: Props = $props();

	let tables = $state<GlobalTable[]>([]);
	let loading = $state(false);
	let createName = $state('');
	let createRegions = $state('');
	let creating = $state(false);
	let regionDrafts = $state<Record<string, string>>({});
	let removeTarget = $state<{ name: string; region: string } | null>(null);
	let removeOpen = $state(false);
	let removeBusy = $state(false);

	async function reload() {
		loading = true;
		try {
			tables = await listGlobalTables();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load global tables');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		if (open) {
			void reload();
		}
	});

	async function onCreate() {
		const name = createName.trim();
		if (!name) {
			toast.error('GlobalTableName is required');
			return;
		}
		const regions = createRegions
			.split(',')
			.map((r) => r.trim())
			.filter((r) => r.length > 0);
		creating = true;
		try {
			await createGlobalTable(name, regions);
			toast.success(`Created global table ${name}`);
			createName = '';
			createRegions = '';
			await reload();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create global table');
		} finally {
			creating = false;
		}
	}

	async function addReplica(name: string) {
		const region = (regionDrafts[name] ?? '').trim();
		if (!region) {
			toast.error('Region name is required');
			return;
		}
		try {
			await updateGlobalTable(name, [{ create: region }]);
			toast.success(`Added replica ${region} to ${name}`);
			regionDrafts[name] = '';
			await reload();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to add replica');
		}
	}

	function removeReplica(name: string, region: string) {
		removeTarget = { name, region };
		removeOpen = true;
	}

	async function confirmRemoveReplica() {
		const t = removeTarget;
		if (!t) return;
		removeBusy = true;
		try {
			await updateGlobalTable(t.name, [{ delete: t.region }]);
			toast.success(`Removed replica ${t.region}`);
			removeOpen = false;
			removeTarget = null;
			await reload();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to remove replica');
		} finally {
			removeBusy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<DialogContent class="max-w-2xl">
		<DialogHeader>
			<DialogTitle>Global Tables</DialogTitle>
			<DialogDescription>
				Cross-region replica configuration. The base table must already exist in the request
				region. AWSim records the replica topology so SDKs and tooling that inspect global
				tables see a consistent view; data is not copied between regions.
			</DialogDescription>
		</DialogHeader>

		<div class="space-y-4">
			<section class="space-y-2 rounded-md border border-border p-3">
				<div class="text-sm font-medium">Create global table</div>
				<div class="grid grid-cols-[1fr_1fr_auto] gap-2">
					<Input placeholder="GlobalTableName" bind:value={createName} />
					<Input
						placeholder="us-east-1, eu-west-1, …"
						bind:value={createRegions}
					/>
					<Button size="sm" onclick={onCreate} disabled={creating}>
						{#if creating}<Loader2 class="size-3.5 animate-spin" />{:else}<Plus class="size-3.5" />{/if}
						Create
					</Button>
				</div>
				<p class="text-[11px] text-muted-foreground">
					Empty regions defaults to the request region.
				</p>
			</section>

			<section class="space-y-2">
				<div class="flex items-center justify-between">
					<div class="text-sm font-medium">
						{tables.length} global table{tables.length === 1 ? '' : 's'}
					</div>
					<Button variant="ghost" size="icon-sm" onclick={reload} disabled={loading}>
						<RefreshCw class={loading ? 'animate-spin size-3.5' : 'size-3.5'} />
					</Button>
				</div>

				{#if tables.length === 0}
					<div class="rounded-md border border-dashed border-border p-4 text-xs text-muted-foreground">
						No global tables defined.
					</div>
				{:else}
					<div class="space-y-2">
						{#each tables as g (g.globalTableName)}
							<div class="rounded-md border border-border p-3">
								<div class="flex items-center justify-between">
									<div class="font-mono text-sm">{g.globalTableName}</div>
									<Badge variant="outline">{g.replicationGroup.length} replica{g.replicationGroup.length === 1 ? '' : 's'}</Badge>
								</div>
								<div class="mt-2 flex flex-wrap gap-1.5">
									{#each g.replicationGroup as r (r.regionName)}
										<button
											type="button"
											onclick={() => removeReplica(g.globalTableName, r.regionName)}
											class="group flex items-center gap-1 rounded border border-border bg-muted/40 px-2 py-0.5 font-mono text-[11px] hover:border-destructive/50 hover:text-destructive"
											title="Remove replica"
										>
											{r.regionName}
											<Trash2 class="size-3 opacity-0 group-hover:opacity-100" />
										</button>
									{/each}
								</div>
								<div class="mt-2 flex items-center gap-1">
									<Input
										placeholder="Add region…"
										bind:value={regionDrafts[g.globalTableName]}
										class="h-7 max-w-[180px] text-xs"
									/>
									<Button size="sm" variant="outline" onclick={() => addReplica(g.globalTableName)}>
										<Plus class="size-3.5" /> Add
									</Button>
								</div>
							</div>
						{/each}
					</div>
				{/if}
			</section>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={onClose}>Close</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>

<ConfirmDialog
	bind:open={removeOpen}
	title="Remove replica?"
	description={`Remove ${removeTarget?.region ?? ''} from ${removeTarget?.name ?? ''}.`}
	confirmLabel="Remove"
	busy={removeBusy}
	onConfirm={confirmRemoveReplica}
	onClose={() => (removeOpen = false)}
/>
