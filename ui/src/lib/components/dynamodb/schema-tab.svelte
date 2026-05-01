<script lang="ts">
	import {
		setDeletionProtection,
		setBillingMode,
		describeTtl,
		updateTtl,
		listTags,
		tagResource,
		untagResource,
		setSse,
		type ResourceTag,
		type TableDetail,
		type TtlState,
	} from '$lib/api/dynamodb';
	import { Badge } from '$lib/components/ui/badge';
	import { Switch } from '$lib/components/ui/switch';
	import { Label } from '$lib/components/ui/label';
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';
	import Save from '@lucide/svelte/icons/save';
	import { toast } from 'svelte-sonner';

	interface Props {
		detail: TableDetail;
		onUpdated?: () => void;
	}

	let { detail, onUpdated }: Props = $props();

	let togglingDeletionProtection = $state(false);
	let savingBillingMode = $state(false);
	let savingSse = $state(false);
	let kmsKeyDraft = $state('');

	// TTL state — loaded async because it has its own DescribeTimeToLive
	// op (the table description doesn't include it).
	let ttl = $state<TtlState>({ enabled: false, attributeName: '' });
	let ttlAttrDraft = $state('');
	let ttlLoaded = $state(false);
	let savingTtl = $state(false);

	// Tags — loaded via ListTagsOfResource.
	let tags = $state<ResourceTag[]>([]);
	let tagDraftKey = $state('');
	let tagDraftValue = $state('');
	let savingTag = $state(false);

	$effect(() => {
		// Reload TTL + tags + KMS key draft whenever the selected
		// table changes.
		const arn = detail.arn;
		const name = detail.name;
		void name;
		ttlLoaded = false;
		tags = [];
		kmsKeyDraft = detail.sse.kmsMasterKeyArn ?? '';
		Promise.all([
			describeTtl(name).catch(() => ({ enabled: false, attributeName: '' })),
			arn ? listTags(arn).catch(() => []) : Promise.resolve([]),
		]).then(([t, tg]) => {
			ttl = t;
			ttlAttrDraft = t.attributeName;
			tags = tg;
			ttlLoaded = true;
		});
	});

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

	async function changeBillingMode(next: 'PAY_PER_REQUEST' | 'PROVISIONED') {
		if (next === detail.billingMode) return;
		savingBillingMode = true;
		try {
			await setBillingMode(detail.name, next);
			toast.success(`Billing mode set to ${next}`);
			onUpdated?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		} finally {
			savingBillingMode = false;
		}
	}

	async function applyTtlEnabled(next: boolean) {
		if (next && !ttlAttrDraft.trim()) {
			toast.error('Set an attribute name before enabling TTL');
			return;
		}
		savingTtl = true;
		try {
			await updateTtl(detail.name, next, ttlAttrDraft.trim());
			ttl = { enabled: next, attributeName: ttlAttrDraft.trim() };
			toast.success(next ? 'TTL enabled' : 'TTL disabled');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		} finally {
			savingTtl = false;
		}
	}

	async function saveTtlAttr() {
		if (!ttlAttrDraft.trim()) {
			toast.error('Attribute name is required');
			return;
		}
		savingTtl = true;
		try {
			await updateTtl(detail.name, true, ttlAttrDraft.trim());
			ttl = { enabled: true, attributeName: ttlAttrDraft.trim() };
			toast.success('TTL attribute updated');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		} finally {
			savingTtl = false;
		}
	}

	async function addTag() {
		const k = tagDraftKey.trim();
		const v = tagDraftValue.trim();
		if (!k) {
			toast.error('Tag key is required');
			return;
		}
		if (!detail.arn) {
			toast.error('Table ARN missing — refresh and retry');
			return;
		}
		savingTag = true;
		try {
			await tagResource(detail.arn, [{ key: k, value: v }]);
			toast.success(`Added tag ${k}`);
			tagDraftKey = '';
			tagDraftValue = '';
			tags = await listTags(detail.arn);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Tag failed');
		} finally {
			savingTag = false;
		}
	}

	async function removeTag(key: string) {
		if (!detail.arn) return;
		try {
			await untagResource(detail.arn, [key]);
			tags = tags.filter((t) => t.key !== key);
			toast.success(`Removed tag ${key}`);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Untag failed');
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

	let ttlAttrModified = $derived(ttlAttrDraft.trim() !== ttl.attributeName.trim());

	async function toggleSse(next: boolean) {
		savingSse = true;
		try {
			await setSse(detail.name, next, kmsKeyDraft.trim() || undefined);
			toast.success(next ? 'SSE enabled' : 'SSE set to AWS-owned key');
			onUpdated?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		} finally {
			savingSse = false;
		}
	}

	async function saveKmsKey() {
		if (!detail.sse.enabled) {
			toast.error('Enable SSE first');
			return;
		}
		savingSse = true;
		try {
			await setSse(detail.name, true, kmsKeyDraft.trim() || undefined);
			toast.success('KMS key updated');
			onUpdated?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		} finally {
			savingSse = false;
		}
	}

	let kmsKeyModified = $derived(
		kmsKeyDraft.trim() !== (detail.sse.kmsMasterKeyArn ?? '').trim()
	);
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
		<div class="space-y-3">
			<!-- Deletion protection -->
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

			<!-- Billing mode -->
			<div class="flex items-start justify-between gap-4 rounded-md border border-border p-3">
				<div class="min-w-0">
					<Label for="ddb-schema-billing-mode" class="text-sm">Billing mode</Label>
					<p class="mt-0.5 text-xs text-muted-foreground">
						<code>PAY_PER_REQUEST</code> bills per call; <code>PROVISIONED</code> uses
						pre-allocated capacity (capacity values are stubbed in awsim).
					</p>
				</div>
				<select
					id="ddb-schema-billing-mode"
					value={detail.billingMode}
					onchange={(e) =>
						changeBillingMode(
							(e.currentTarget as HTMLSelectElement).value as
								| 'PAY_PER_REQUEST'
								| 'PROVISIONED'
						)}
					disabled={savingBillingMode}
					class="h-9 rounded-md border border-border bg-background px-2 text-xs disabled:opacity-50"
				>
					<option value="PAY_PER_REQUEST">PAY_PER_REQUEST</option>
					<option value="PROVISIONED">PROVISIONED</option>
				</select>
			</div>

			<!-- TTL -->
			<div class="rounded-md border border-border p-3">
				<div class="flex items-start justify-between gap-4">
					<div class="min-w-0">
						<Label for="ddb-schema-ttl" class="text-sm">Time to live (TTL)</Label>
						<p class="mt-0.5 text-xs text-muted-foreground">
							Items whose <code>{ttl.attributeName || 'TTL attribute'}</code> is a Unix timestamp ≤ now
							get deleted by the background sweeper (runs once per minute).
						</p>
					</div>
					<Switch
						id="ddb-schema-ttl"
						checked={ttl.enabled}
						onCheckedChange={(v) => applyTtlEnabled(v)}
						disabled={!ttlLoaded || savingTtl}
					/>
				</div>
				<div class="mt-3 flex items-end gap-2">
					<div class="flex-1">
						<Label class="text-xs text-muted-foreground">Attribute name</Label>
						<Input
							bind:value={ttlAttrDraft}
							placeholder="e.g. expires_at"
							disabled={!ttlLoaded || savingTtl}
							class="h-8 font-mono text-xs"
						/>
					</div>
					{#if ttl.enabled && ttlAttrModified}
						<Button size="sm" onclick={saveTtlAttr} disabled={savingTtl}>
							<Save class="size-3.5" />
							<span class="ml-1">Save</span>
						</Button>
					{/if}
				</div>
			</div>

			<!-- Tags -->
			<div class="rounded-md border border-border p-3">
				<div class="mb-2 flex items-center justify-between">
					<div>
						<div class="text-sm font-medium">Tags</div>
						<p class="mt-0.5 text-xs text-muted-foreground">
							Key/value pairs. Used by AWS Resource Groups Tagging and visible in cost
							reports.
						</p>
					</div>
					<Badge variant="outline">{tags.length}</Badge>
				</div>
				{#if tags.length > 0}
					<ul class="space-y-1">
						{#each tags as t (t.key)}
							<li
								class="flex items-center gap-2 rounded border border-border/60 px-2 py-1.5"
							>
								<span class="font-mono text-xs">{t.key}</span>
								<span class="text-muted-foreground">=</span>
								<span class="flex-1 truncate font-mono text-xs">{t.value}</span>
								<Button
									variant="ghost"
									size="icon-sm"
									onclick={() => removeTag(t.key)}
									aria-label="Remove tag"
								>
									<X class="size-3.5" />
								</Button>
							</li>
						{/each}
					</ul>
				{:else}
					<p class="text-xs text-muted-foreground">No tags.</p>
				{/if}
				<div class="mt-2 flex items-end gap-2">
					<div class="flex-1">
						<Label class="text-xs text-muted-foreground">Key</Label>
						<Input bind:value={tagDraftKey} placeholder="env" class="h-8 font-mono text-xs" />
					</div>
					<div class="flex-1">
						<Label class="text-xs text-muted-foreground">Value</Label>
						<Input
							bind:value={tagDraftValue}
							placeholder="prod"
							class="h-8 font-mono text-xs"
						/>
					</div>
					<Button size="sm" onclick={addTag} disabled={savingTag || !tagDraftKey.trim()}>
						<Plus class="size-3.5" />
						<span class="ml-1">Add</span>
					</Button>
				</div>
			</div>

			<!-- SSE / encryption -->
			<div class="rounded-md border border-border p-3">
				<div class="flex items-start justify-between gap-4">
					<div class="min-w-0">
						<Label for="ddb-schema-sse" class="text-sm">Encryption (SSE)</Label>
						<p class="mt-0.5 text-xs text-muted-foreground">
							Off uses AWS-owned keys (the default — invisible in DescribeTable). On reports
							customer-managed KMS encryption. awsim doesn't actually encrypt items; the
							setting round-trips through the API for SDK code that reads it.
						</p>
					</div>
					<Switch
						id="ddb-schema-sse"
						checked={detail.sse.enabled}
						onCheckedChange={(v) => toggleSse(v)}
						disabled={savingSse}
					/>
				</div>
				{#if detail.sse.enabled}
					<div class="mt-3 flex items-end gap-2">
						<div class="flex-1">
							<Label class="text-xs text-muted-foreground">KMS key ARN (optional)</Label>
							<Input
								bind:value={kmsKeyDraft}
								placeholder="arn:aws:kms:us-east-1:000000000000:key/…"
								disabled={savingSse}
								class="h-8 font-mono text-xs"
							/>
						</div>
						{#if kmsKeyModified}
							<Button size="sm" onclick={saveKmsKey} disabled={savingSse}>
								<Save class="size-3.5" />
								<span class="ml-1">Save</span>
							</Button>
						{/if}
					</div>
					<div class="mt-2 flex items-center gap-2 text-xs text-muted-foreground">
						<span class="uppercase tracking-wide">Type</span>
						<Badge variant="outline" class="font-mono">
							{detail.sse.sseType || 'KMS'}
						</Badge>
					</div>
				{/if}
			</div>
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
