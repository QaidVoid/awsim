<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		describeUserPool,
		describeDomain,
		createDomain,
		deleteDomain,
		type CognitoDomain,
		type UserPoolDetail
	} from '$lib/api/cognito';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';

	interface Props {
		poolId: string;
	}

	let { poolId }: Props = $props();

	let detail = $state<UserPoolDetail | null>(null);
	let domain = $state<CognitoDomain | null>(null);
	let loading = $state(true);
	let domainInput = $state('');
	let busy = $state(false);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	onMount(load);

	async function load() {
		loading = true;
		try {
			detail = await describeUserPool(poolId);
			if (detail.domain) {
				try {
					domain = (await describeDomain(detail.domain)) ?? { domain: detail.domain };
				} catch {
					domain = { domain: detail.domain };
				}
			} else {
				domain = null;
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load domain');
		} finally {
			loading = false;
		}
	}

	async function submit() {
		if (!domainInput.trim()) return;
		busy = true;
		try {
			await createDomain(poolId, domainInput.trim());
			toast.success(`Domain ${domainInput.trim()} created`);
			domainInput = '';
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Create domain failed');
		} finally {
			busy = false;
		}
	}

	async function confirmDelete() {
		if (!domain) return;
		deleteBusy = true;
		try {
			await deleteDomain(poolId, domain.domain);
			toast.success('Domain deleted');
			deleteOpen = false;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteBusy = false;
		}
	}
</script>

<div class="space-y-4 px-6 py-4">
	{#if loading}
		<p class="text-xs text-muted-foreground">Loading...</p>
	{:else}
		<dl class="grid grid-cols-3 gap-x-4 gap-y-2 text-sm">
			<dt class="text-muted-foreground">Status</dt>
			<dd class="col-span-2">{detail?.status ?? '—'}</dd>
			<dt class="text-muted-foreground">Created</dt>
			<dd class="col-span-2">{detail?.creationDate ?? '—'}</dd>
			<dt class="text-muted-foreground">MFA</dt>
			<dd class="col-span-2">{detail?.mfaConfiguration ?? 'OFF'}</dd>
			<dt class="text-muted-foreground">Estimated users</dt>
			<dd class="col-span-2">{detail?.estimatedNumberOfUsers ?? 0}</dd>
		</dl>

		<div class="space-y-2 rounded border border-border/60 px-3 py-3">
			<div class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
				Hosted UI domain
			</div>
			{#if domain}
				<div class="flex flex-wrap items-center gap-2 text-sm">
					<code class="font-mono text-xs">{domain.domain}</code>
					{#if domain.status}
						<Badge variant="outline">{domain.status}</Badge>
					{/if}
					<div class="flex-1"></div>
					<Button
						variant="ghost"
						size="xs"
						class="text-destructive hover:text-destructive"
						onclick={() => (deleteOpen = true)}
					>
						Delete
					</Button>
				</div>
			{:else}
				<form
					class="flex items-end gap-2"
					onsubmit={(e) => {
						e.preventDefault();
						void submit();
					}}
				>
					<div class="flex-1 space-y-1">
						<label for="domain-input" class="text-xs text-muted-foreground">
							Domain prefix
						</label>
						<Input
							id="domain-input"
							bind:value={domainInput}
							placeholder="my-pool"
							class="h-8 font-mono text-xs"
							autocomplete="off"
						/>
					</div>
					<Button size="sm" type="submit" disabled={busy || !domainInput.trim()}>Create</Button>
				</form>
			{/if}
		</div>
	{/if}
</div>

{#if domain}
	<ConfirmDialog
		bind:open={deleteOpen}
		title="Delete domain"
		description={`Delete the hosted UI domain ${domain.domain}? Sign-ins to it will stop working.`}
		busy={deleteBusy}
		onConfirm={confirmDelete}
		onClose={() => (deleteOpen = false)}
	/>
{/if}
