<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		listAppClients,
		getUiCustomization,
		setUiCustomization,
		type CognitoAppClient
	} from '$lib/api/cognito';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		poolId: string;
	}

	let { poolId }: Props = $props();

	const POOL_DEFAULT = '__POOL__';

	let clients = $state<CognitoAppClient[]>([]);
	let scope = $state(POOL_DEFAULT);
	let css = $state('');
	let imageUrl = $state('');
	let original = $state<{ css: string; imageUrl: string } | null>(null);
	let loading = $state(true);
	let saving = $state(false);

	const dirty = $derived.by(() => {
		if (!original) return false;
		return css !== original.css || imageUrl !== original.imageUrl;
	});

	onMount(loadAll);

	async function loadAll() {
		loading = true;
		try {
			const cl = await listAppClients(poolId, { maxResults: 60 });
			clients = cl.clients;
			await loadCustomization();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load appearance');
		} finally {
			loading = false;
		}
	}

	async function loadCustomization() {
		try {
			const u = await getUiCustomization(
				poolId,
				scope === POOL_DEFAULT ? undefined : scope
			);
			css = u.css ?? '';
			imageUrl = u.imageUrl ?? '';
			original = { css, imageUrl };
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load customization');
		}
	}

	$effect(() => {
		// Switch the loaded customization when the scope changes.
		scope;
		void loadCustomization();
	});

	async function save() {
		saving = true;
		try {
			await setUiCustomization({
				poolId,
				clientId: scope === POOL_DEFAULT ? undefined : scope,
				css,
				imageUrl
			});
			toast.success('Customization saved');
			await loadCustomization();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Save failed');
		} finally {
			saving = false;
		}
	}

	function reset() {
		if (!original) return;
		css = original.css;
		imageUrl = original.imageUrl;
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div
		class="sticky top-0 z-10 flex flex-wrap items-center gap-2 border-b border-border bg-background px-6 py-3"
	>
		<Label for="scope-select" class="text-xs">Scope</Label>
		<select
			id="scope-select"
			bind:value={scope}
			class="h-8 rounded-md border border-border bg-background px-2 text-xs"
		>
			<option value={POOL_DEFAULT}>Pool default (all clients)</option>
			{#each clients as c (c.clientId)}
				<option value={c.clientId}>{c.clientName} — {c.clientId}</option>
			{/each}
		</select>
		<div class="flex-1"></div>
		<Button variant="ghost" size="sm" onclick={reset} disabled={saving || !dirty}>
			Discard
		</Button>
		<Button size="sm" onclick={save} disabled={saving || !dirty}>
			{#if saving}<Loader2 class="size-3.5 animate-spin" />{/if}
			Save
		</Button>
	</div>

	<div class="flex-1 space-y-4 overflow-y-auto px-6 py-4">
		{#if loading}
			<p class="text-xs text-muted-foreground">Loading...</p>
		{:else}
			<p class="text-xs text-muted-foreground">
				Customizes the Cognito hosted UI sign-in / sign-up pages. Pool default applies when no
				per-client customization is set. Awsim's hosted UI doesn't apply this CSS yet — values
				are stored for SDK round-trip parity.
			</p>

			<div class="space-y-1.5">
				<Label for="logo-url">Logo URL or data URI</Label>
				<Input
					id="logo-url"
					bind:value={imageUrl}
					placeholder="https://example.com/logo.png or data:image/png;base64,..."
					class="font-mono text-xs"
				/>
				{#if imageUrl}
					<div class="flex items-center gap-2 rounded border border-border/60 bg-muted/40 p-2">
						<img src={imageUrl} alt="logo preview" class="h-12 w-auto object-contain" />
						<span class="text-xs text-muted-foreground">Preview</span>
					</div>
				{/if}
			</div>

			<div class="space-y-1.5">
				<Label for="css">CSS</Label>
				<textarea
					id="css"
					bind:value={css}
					placeholder={`.banner-customizable { background: linear-gradient(...); }\n.label-customizable { ... }`}
					class="block min-h-[400px] w-full resize-y rounded border border-border bg-background px-3 py-2 font-mono text-xs"
				></textarea>
			</div>
		{/if}
	</div>
</div>
