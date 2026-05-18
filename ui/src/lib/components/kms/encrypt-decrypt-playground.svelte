<script lang="ts">
	import { onMount } from 'svelte';
	import { listKeys, encrypt, decrypt, type Key } from '$lib/api/kms';
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Badge } from '$lib/components/ui/badge';
	import Lock from '@lucide/svelte/icons/lock';
	import Unlock from '@lucide/svelte/icons/unlock';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import { toast } from 'svelte-sonner';

	let keys = $state<Key[]>([]);
	let keyId = $state('');
	let plaintext = $state('hello kms');
	let ciphertext = $state('');
	let decrypted = $state('');
	let busy = $state(false);
	let verified = $state<boolean | null>(null);

	async function loadKeys() {
		try {
			keys = await listKeys();
			if (!keyId && keys.length > 0) keyId = keys[0].keyId;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load keys');
		}
	}

	async function runEncrypt() {
		if (!keyId) {
			toast.error('Pick a key first');
			return;
		}
		busy = true;
		ciphertext = '';
		decrypted = '';
		verified = null;
		try {
			ciphertext = await encrypt(keyId, plaintext);
			toast.success('Encrypted');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Encrypt failed');
		} finally {
			busy = false;
		}
	}

	async function runDecrypt() {
		if (!ciphertext) {
			toast.error('Encrypt something first');
			return;
		}
		busy = true;
		try {
			decrypted = await decrypt(ciphertext);
			verified = decrypted === plaintext;
			if (verified) toast.success('Round-trip verified');
			else toast.warning('Decrypted, but does not match original plaintext');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Decrypt failed');
		} finally {
			busy = false;
		}
	}

	onMount(loadKeys);
</script>

<Card class="m-6">
	<CardHeader>
		<CardTitle class="flex items-center gap-2">
			<Lock class="size-4 text-primary" /> Encrypt / decrypt playground
		</CardTitle>
	</CardHeader>
	<CardContent class="grid gap-4">
		<div class="grid gap-3 md:grid-cols-2">
			<div class="flex flex-col gap-1.5">
				<Label for="kms-key" class="text-xs">Key</Label>
				<Select type="single" bind:value={keyId}>
					<SelectTrigger id="kms-key" class="w-full">
						{keyId || 'No keys found'}
					</SelectTrigger>
					<SelectContent>
						{#each keys as k (k.keyId)}
							<SelectItem value={k.keyId} label={k.keyId}>{k.keyId}</SelectItem>
						{/each}
					</SelectContent>
				</Select>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="kms-plaintext" class="text-xs">Plaintext</Label>
				<Input id="kms-plaintext" bind:value={plaintext} class="font-mono text-xs" />
			</div>
		</div>
		<div class="flex flex-wrap items-center gap-2">
			<Button onclick={runEncrypt} disabled={busy}>
				{#if busy}<Loader2 class="size-4 animate-spin" />{:else}<Lock class="size-4" />{/if}
				Encrypt
			</Button>
			<Button variant="outline" onclick={runDecrypt} disabled={busy || !ciphertext}>
				{#if busy}<Loader2 class="size-4 animate-spin" />{:else}<Unlock class="size-4" />{/if}
				Decrypt
			</Button>
			{#if verified === true}
				<Badge variant="secondary" class="bg-emerald-500/15 text-emerald-500"
					>Round-trip verified</Badge
				>
			{:else if verified === false}
				<Badge variant="destructive">Mismatch</Badge>
			{/if}
		</div>
		{#if ciphertext}
			<div class="flex flex-col gap-1.5">
				<Label for="kms-cipher" class="text-xs">Ciphertext (base64)</Label>
				<Textarea
					id="kms-cipher"
					readonly
					value={ciphertext}
					rows={3}
					class="font-mono text-xs"
				/>
			</div>
		{/if}
		{#if decrypted}
			<div class="flex flex-col gap-1.5">
				<Label for="kms-plain" class="text-xs">Decrypted plaintext</Label>
				<Textarea id="kms-plain" readonly value={decrypted} rows={2} class="font-mono text-xs" />
			</div>
		{/if}
	</CardContent>
</Card>
