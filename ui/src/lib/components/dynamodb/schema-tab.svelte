<script lang="ts">
	import { setDeletionProtection, type TableDetail } from '$lib/api/dynamodb';
	import { Badge } from '$lib/components/ui/badge';
	import { Switch } from '$lib/components/ui/switch';
	import { Label } from '$lib/components/ui/label';
	import { toast } from 'svelte-sonner';

	interface Props {
		detail: TableDetail;
		onUpdated?: () => void;
	}

	let { detail, onUpdated }: Props = $props();

	let togglingDeletionProtection = $state(false);

	async function toggleDeletionProtection(next: boolean) {
		togglingDeletionProtection = true;
		try {
			await setDeletionProtection(detail.name, next);
			toast.success(
				next ? 'Deletion protection enabled' : 'Deletion protection disabled'
			);
			onUpdated?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		} finally {
			togglingDeletionProtection = false;
		}
	}

	function formatBytes(n: number): string {
		if (!n) return '0 B';
		const units = ['B', 'KB', 'MB', 'GB'];
		let value = n;
		let i = 0;
		while (value >= 1024 && i < units.length - 1) {
			value /= 1024;
			i++;
		}
		const rounded = value >= 100 ? Math.round(value) : Math.round(value * 10) / 10;
		return `${rounded} ${units[i]}`;
	}
</script>

<div class="flex h-full min-h-0 flex-col gap-6 overflow-y-auto p-4">
	<section>
		<h3 class="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
			Overview
		</h3>
		<dl class="grid grid-cols-[140px_1fr] gap-y-1.5 text-xs">
			<dt class="text-muted-foreground">Status</dt>
			<dd>
				<Badge variant={detail.status === 'ACTIVE' ? 'secondary' : 'outline'}>
					{detail.status || 'UNKNOWN'}
				</Badge>
			</dd>

			<dt class="text-muted-foreground">Items</dt>
			<dd class="font-mono">{detail.itemCount.toLocaleString()}</dd>

			<dt class="text-muted-foreground">Size</dt>
			<dd class="font-mono">{formatBytes(detail.tableSizeBytes)}</dd>

			<dt class="text-muted-foreground">Billing</dt>
			<dd class="font-mono">{detail.billingMode}</dd>

			{#if detail.createdAt}
				<dt class="text-muted-foreground">Created</dt>
				<dd class="font-mono text-[11px]">
					{new Date(detail.createdAt).toLocaleString()}
				</dd>
			{/if}
		</dl>
	</section>

	<section>
		<h3 class="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
			Settings
		</h3>
		<div class="flex items-start justify-between gap-4 rounded-md border border-border p-3">
			<div class="min-w-0">
				<Label for="ddb-schema-deletion-protection" class="text-sm">
					Deletion protection
				</Label>
				<p class="mt-0.5 text-xs text-muted-foreground">
					When on, <code>DeleteTable</code> rejects the request. Disable here before deleting.
				</p>
			</div>
			<Switch
				id="ddb-schema-deletion-protection"
				checked={detail.deletionProtectionEnabled}
				onCheckedChange={(v) => toggleDeletionProtection(v)}
				disabled={togglingDeletionProtection}
			/>
		</div>
	</section>

	<section>
		<h3 class="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
			Key schema
		</h3>
		<table class="w-full text-xs">
			<thead>
				<tr class="border-b border-border text-left text-muted-foreground">
					<th class="py-1.5 pr-4 font-medium">Attribute</th>
					<th class="py-1.5 pr-4 font-medium">Role</th>
					<th class="py-1.5 font-medium">Type</th>
				</tr>
			</thead>
			<tbody>
				{#each detail.keySchema as k (k.attributeName)}
					{@const def = detail.attributeDefinitions.find(
						(a) => a.attributeName === k.attributeName
					)}
					<tr class="border-b border-border/30">
						<td class="py-1.5 pr-4 font-mono">{k.attributeName}</td>
						<td class="py-1.5 pr-4">
							<Badge variant="outline">
								{k.keyType === 'HASH' ? 'Partition' : 'Sort'}
							</Badge>
						</td>
						<td class="py-1.5 font-mono">{def?.attributeType ?? '—'}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</section>

	{#if detail.attributeDefinitions.length > 0}
		<section>
			<h3 class="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
				Attribute definitions
			</h3>
			<table class="w-full text-xs">
				<thead>
					<tr class="border-b border-border text-left text-muted-foreground">
						<th class="py-1.5 pr-4 font-medium">Attribute</th>
						<th class="py-1.5 font-medium">Type</th>
					</tr>
				</thead>
				<tbody>
					{#each detail.attributeDefinitions as a (a.attributeName)}
						<tr class="border-b border-border/30">
							<td class="py-1.5 pr-4 font-mono">{a.attributeName}</td>
							<td class="py-1.5 font-mono">{a.attributeType}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</section>
	{/if}
</div>
