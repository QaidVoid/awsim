<script lang="ts">
	/**
	 * Concurrency tab — surfaces both reserved (per-function ceiling) and
	 * provisioned (warm-pool) concurrency configurations. Real Lambda gates
	 * invocations against the reserved value; AWSim stores it round-trip but
	 * doesn't actually throttle. Provisioned configs flip IN_PROGRESS ->
	 * READY immediately because there's no real warm pool to provision.
	 */
	import { onMount } from 'svelte';
	import {
		deleteFunctionConcurrency,
		deleteProvisionedConcurrencyConfig,
		getFunctionConcurrency,
		listProvisionedConcurrencyConfigs,
		putFunctionConcurrency,
		putProvisionedConcurrencyConfig,
		type ProvisionedConcurrencyConfig
	} from '$lib/api/lambda';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import { toast } from 'svelte-sonner';

	interface Props {
		functionName: string;
	}
	let { functionName }: Props = $props();

	let reserved = $state<number | undefined>(undefined);
	let reservedDraft = $state('');
	let savingReserved = $state(false);

	let provisioned = $state<ProvisionedConcurrencyConfig[]>([]);
	let loading = $state(true);
	let savingProvisioned = $state(false);
	let newQualifier = $state('');
	let newCount = $state(1);

	let removeTarget = $state<string | null>(null);
	let removeOpen = $state(false);
	let removeBusy = $state(false);

	async function reload() {
		loading = true;
		try {
			const [r, p] = await Promise.all([
				getFunctionConcurrency(functionName),
				listProvisionedConcurrencyConfigs(functionName)
			]);
			reserved = r.reservedConcurrentExecutions;
			reservedDraft = reserved !== undefined ? String(reserved) : '';
			provisioned = p;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load concurrency');
		} finally {
			loading = false;
		}
	}

	async function saveReserved() {
		const n = parseInt(reservedDraft.trim(), 10);
		if (Number.isNaN(n) || n < 0) {
			toast.error('Reserved concurrent executions must be a non-negative integer');
			return;
		}
		savingReserved = true;
		try {
			await putFunctionConcurrency(functionName, n);
			toast.success('Reserved concurrency saved');
			await reload();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to save reserved concurrency');
		} finally {
			savingReserved = false;
		}
	}

	async function clearReserved() {
		savingReserved = true;
		try {
			await deleteFunctionConcurrency(functionName);
			toast.success('Reserved concurrency cleared');
			await reload();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to clear reserved concurrency');
		} finally {
			savingReserved = false;
		}
	}

	async function addProvisioned() {
		const qualifier = newQualifier.trim();
		if (!qualifier) {
			toast.error('Qualifier (alias name or version) is required');
			return;
		}
		if (qualifier === '$LATEST') {
			toast.error('Provisioned concurrency cannot target $LATEST');
			return;
		}
		if (newCount < 1) {
			toast.error('ProvisionedConcurrentExecutions must be at least 1');
			return;
		}
		savingProvisioned = true;
		try {
			await putProvisionedConcurrencyConfig(functionName, qualifier, newCount);
			toast.success(`Provisioned ${newCount} for ${qualifier}`);
			newQualifier = '';
			newCount = 1;
			await reload();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to provision');
		} finally {
			savingProvisioned = false;
		}
	}

	function removeProvisioned(qualifier: string) {
		removeTarget = qualifier;
		removeOpen = true;
	}

	async function confirmRemoveProvisioned() {
		const qualifier = removeTarget;
		if (qualifier === null) return;
		removeBusy = true;
		try {
			await deleteProvisionedConcurrencyConfig(functionName, qualifier);
			toast.success(`Removed provisioned concurrency for ${qualifier}`);
			removeOpen = false;
			removeTarget = null;
			await reload();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to remove');
		} finally {
			removeBusy = false;
		}
	}

	function statusVariant(status: string): 'default' | 'secondary' | 'destructive' | 'outline' {
		if (status === 'READY') return 'default';
		if (status === 'IN_PROGRESS') return 'secondary';
		if (status === 'FAILED') return 'destructive';
		return 'outline';
	}

	$effect(() => {
		// Re-load when the selected function changes.
		if (functionName) reload();
	});
</script>

<div class="flex h-full min-h-0 flex-col gap-4 p-4">
	<section class="space-y-2">
		<div class="flex items-center justify-between">
			<h3 class="text-sm font-medium">Reserved concurrency</h3>
			<Button variant="ghost" size="icon-sm" onclick={reload} disabled={loading}>
				<RefreshCw class={loading ? 'animate-spin size-3.5' : 'size-3.5'} />
			</Button>
		</div>
		<p class="text-xs text-muted-foreground">
			Caps the number of concurrent executions for this function. Real Lambda gates
			invocations against this value; AWSim stores and returns it for SDK code that reads
			it, without enforcing the limit.
		</p>
		{#if loading && reserved === undefined}
			<Skeleton class="h-9 w-64" />
		{:else}
			<div class="flex items-center gap-2">
				<Input
					type="number"
					min="0"
					max="1000"
					placeholder="Unreserved"
					bind:value={reservedDraft}
					class="max-w-[160px]"
				/>
				<Button size="sm" onclick={saveReserved} disabled={savingReserved}>Save</Button>
				{#if reserved !== undefined}
					<Button variant="ghost" size="sm" onclick={clearReserved} disabled={savingReserved}>
						Clear reservation
					</Button>
				{/if}
			</div>
		{/if}
	</section>

	<section class="space-y-2">
		<h3 class="text-sm font-medium">Provisioned concurrency</h3>
		<p class="text-xs text-muted-foreground">
			Per-(function, qualifier) warm-pool sizing. Targets a published version or alias —
			never <code>$LATEST</code>. AWSim flips status to READY immediately.
		</p>

		<div class="flex items-end gap-2">
			<div class="flex flex-col gap-1">
				<label class="text-[11px] text-muted-foreground" for="qualifier">Qualifier</label>
				<Input
					id="qualifier"
					bind:value={newQualifier}
					placeholder="alias or version"
					class="max-w-[200px]"
				/>
			</div>
			<div class="flex flex-col gap-1">
				<label class="text-[11px] text-muted-foreground" for="count">Count</label>
				<Input
					id="count"
					type="number"
					min="1"
					bind:value={newCount}
					class="max-w-[120px]"
				/>
			</div>
			<Button size="sm" onclick={addProvisioned} disabled={savingProvisioned}>
				Provision
			</Button>
		</div>

		{#if loading && provisioned.length === 0}
			<Skeleton class="h-12 w-full" />
		{:else if provisioned.length === 0}
			<div class="rounded-md border border-dashed border-border p-4 text-xs text-muted-foreground">
				No provisioned concurrency configurations.
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="border-b border-border text-left">
					<tr>
						<th class="px-3 py-2 text-xs font-medium text-muted-foreground">Qualifier</th>
						<th class="px-3 py-2 text-xs font-medium text-muted-foreground">Status</th>
						<th class="px-3 py-2 text-right text-xs font-medium text-muted-foreground">Requested</th>
						<th class="px-3 py-2 text-right text-xs font-medium text-muted-foreground">Available</th>
						<th class="px-3 py-2 text-right text-xs font-medium text-muted-foreground">Last Modified</th>
						<th class="px-3 py-2 text-right text-xs font-medium text-muted-foreground"></th>
					</tr>
				</thead>
				<tbody>
					{#each provisioned as p (p.qualifier)}
						<tr class="border-b border-border/40">
							<td class="px-3 py-2 font-mono text-xs">{p.qualifier || '—'}</td>
							<td class="px-3 py-2"><Badge variant={statusVariant(p.status)}>{p.status}</Badge></td>
							<td class="px-3 py-2 text-right font-mono text-xs">
								{p.requestedProvisionedConcurrentExecutions}
							</td>
							<td class="px-3 py-2 text-right font-mono text-xs">
								{p.availableProvisionedConcurrentExecutions}
							</td>
							<td class="px-3 py-2 text-right text-xs text-muted-foreground">
								{p.lastModified}
							</td>
							<td class="px-3 py-2 text-right">
								<Button
									variant="ghost"
									size="icon-xs"
									onclick={() => removeProvisioned(p.qualifier)}
									aria-label="Remove provisioned config"
								>
									<Trash2 class="size-3.5" />
								</Button>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</section>
</div>

<ConfirmDialog
	bind:open={removeOpen}
	title="Remove provisioned concurrency?"
	description={`Remove provisioned concurrency for ${removeTarget ?? ''}.`}
	confirmLabel="Remove"
	busy={removeBusy}
	onConfirm={confirmRemoveProvisioned}
	onClose={() => (removeOpen = false)}
/>
