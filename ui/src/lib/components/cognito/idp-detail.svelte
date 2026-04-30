<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		describeIdentityProvider,
		updateIdentityProvider,
		type IdentityProviderDetail
	} from '$lib/api/cognito';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import KvEditor from './kv-editor.svelte';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		poolId: string;
		name: string;
	}

	let { poolId, name }: Props = $props();

	let original = $state<IdentityProviderDetail | null>(null);
	let details = $state<{ key: string; value: string }[]>([]);
	let attrs = $state<{ key: string; value: string }[]>([]);
	let loading = $state(true);
	let saving = $state(false);

	const dirty = $derived.by(() => {
		if (!original) return false;
		const detailsMap = Object.fromEntries(details.map((d) => [d.key, d.value]));
		const attrsMap = Object.fromEntries(attrs.map((a) => [a.key, a.value]));
		return (
			JSON.stringify(detailsMap) !== JSON.stringify(original.providerDetails) ||
			JSON.stringify(attrsMap) !== JSON.stringify(original.attributeMapping)
		);
	});

	onMount(load);

	async function load() {
		loading = true;
		try {
			const d = await describeIdentityProvider(poolId, name);
			original = d;
			details = Object.entries(d.providerDetails).map(([key, value]) => ({ key, value }));
			attrs = Object.entries(d.attributeMapping).map(([key, value]) => ({ key, value }));
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load provider');
		} finally {
			loading = false;
		}
	}

	async function save() {
		saving = true;
		try {
			const detailsMap = Object.fromEntries(details.map((d) => [d.key, d.value]));
			const attrsMap = Object.fromEntries(attrs.map((a) => [a.key, a.value]));
			await updateIdentityProvider({
				poolId,
				name,
				providerDetails: detailsMap,
				attributeMapping: attrsMap
			});
			toast.success('Provider saved');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Save failed');
		} finally {
			saving = false;
		}
	}

	function reset() {
		if (!original) return;
		details = Object.entries(original.providerDetails).map(([key, value]) => ({ key, value }));
		attrs = Object.entries(original.attributeMapping).map(([key, value]) => ({ key, value }));
	}
</script>

<div class="space-y-4 rounded border border-border/60 bg-muted/20 px-3 py-3">
	{#if loading}
		<p class="text-xs text-muted-foreground">
			<Loader2 class="inline size-3 animate-spin" /> Loading...
		</p>
	{:else}
		<div>
			<Label class="text-xs uppercase tracking-wide text-muted-foreground">
				Provider details
			</Label>
			<KvEditor bind:entries={details} onChange={(e) => (details = e)} />
		</div>
		<div>
			<Label class="text-xs uppercase tracking-wide text-muted-foreground">
				Attribute mapping
			</Label>
			<KvEditor
				bind:entries={attrs}
				keyPlaceholder="cognito attr"
				valuePlaceholder="idp attr"
				onChange={(e) => (attrs = e)}
			/>
		</div>
		<div class="flex justify-end gap-2">
			<Button variant="ghost" size="sm" onclick={reset} disabled={saving || !dirty}>
				Discard
			</Button>
			<Button size="sm" onclick={save} disabled={saving || !dirty}>
				{#if saving}<Loader2 class="size-3.5 animate-spin" />{/if}
				Save
			</Button>
		</div>
	{/if}
</div>
