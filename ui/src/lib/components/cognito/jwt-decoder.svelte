<script lang="ts">
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import KeyRound from '@lucide/svelte/icons/key-round';

	interface Props {
		/** Bindable so callers (e.g. the sign-in flow) can push a token in. */
		token?: string;
	}

	let { token = $bindable('') }: Props = $props();

	function base64UrlDecode(s: string): string {
		const pad = s.length % 4;
		const padded = s + (pad ? '='.repeat(4 - pad) : '');
		const normalized = padded.replace(/-/g, '+').replace(/_/g, '/');
		try {
			return decodeURIComponent(
				atob(normalized)
					.split('')
					.map((c) => '%' + ('00' + c.charCodeAt(0).toString(16)).slice(-2))
					.join('')
			);
		} catch {
			try {
				return atob(normalized);
			} catch {
				return '';
			}
		}
	}

	function safeJson(s: string): string {
		try {
			return JSON.stringify(JSON.parse(s), null, 2);
		} catch {
			return s;
		}
	}

	const parts = $derived(token.split('.'));
	const headerJson = $derived(parts.length >= 2 ? safeJson(base64UrlDecode(parts[0])) : '');
	const payloadJson = $derived(parts.length >= 2 ? safeJson(base64UrlDecode(parts[1])) : '');
	const signature = $derived(parts.length >= 3 ? parts[2] : '');

	const claims = $derived.by((): Record<string, unknown> => {
		try {
			return JSON.parse(payloadJson) as Record<string, unknown>;
		} catch {
			return {};
		}
	});
	const expClaim = $derived(typeof claims.exp === 'number' ? (claims.exp as number) : undefined);
	const nowSec = $derived(Math.floor(Date.now() / 1000));
	const expired = $derived(expClaim !== undefined && expClaim < nowSec);
</script>

<Card class="m-6">
	<CardHeader>
		<CardTitle class="flex items-center gap-2">
			<KeyRound class="size-4 text-primary" /> JWT decoder
		</CardTitle>
	</CardHeader>
	<CardContent class="grid gap-4">
		<div class="flex flex-col gap-1.5">
			<Label for="jwt-token" class="text-xs">Paste a JWT (id / access / refresh token)</Label>
			<Textarea
				id="jwt-token"
				bind:value={token}
				rows={3}
				placeholder="eyJraWQiOi...."
				class="font-mono text-xs"
			/>
		</div>
		{#if token.trim()}
			{#if parts.length < 2}
				<p class="text-xs text-destructive">Not a JWT — expected at least header.payload.</p>
			{:else}
				<div class="flex flex-wrap items-center gap-1.5 text-xs">
					{#if expClaim !== undefined}
						{#if expired}
							<Badge variant="destructive">EXPIRED</Badge>
						{:else}
							<Badge variant="secondary">valid until {new Date(expClaim * 1000).toLocaleString()}</Badge>
						{/if}
					{/if}
					{#if claims.iss}
						<Badge variant="outline">iss: {String(claims.iss)}</Badge>
					{/if}
					{#if claims.token_use}
						<Badge variant="outline">token_use: {String(claims.token_use)}</Badge>
					{/if}
					{#if claims.client_id}
						<Badge variant="outline">client: {String(claims.client_id)}</Badge>
					{/if}
				</div>
				<div class="grid gap-3 lg:grid-cols-2">
					<div>
						<Label class="text-xs uppercase tracking-wide text-muted-foreground" for="jwt-header"
							>Header</Label
						>
						<pre
							id="jwt-header"
							class="mt-1 max-h-48 overflow-auto rounded border border-border/60 bg-muted/30 p-2 font-mono text-xs">{headerJson}</pre>
					</div>
					<div>
						<Label class="text-xs uppercase tracking-wide text-muted-foreground" for="jwt-payload"
							>Payload</Label
						>
						<pre
							id="jwt-payload"
							class="mt-1 max-h-48 overflow-auto rounded border border-border/60 bg-muted/30 p-2 font-mono text-xs">{payloadJson}</pre>
					</div>
				</div>
				{#if signature}
					<div>
						<Label class="text-xs uppercase tracking-wide text-muted-foreground" for="jwt-sig"
							>Signature (base64url, not verified)</Label
						>
						<div
							id="jwt-sig"
							class="mt-1 max-h-24 overflow-auto rounded border border-border/60 bg-muted/30 p-2 font-mono text-xs break-all"
						>
							{signature}
						</div>
					</div>
				{/if}
			{/if}
		{:else}
			<p class="text-xs text-muted-foreground">
				Paste a token above to decode its header, payload, and signature segments. No data leaves
				the browser.
			</p>
		{/if}
	</CardContent>
</Card>
