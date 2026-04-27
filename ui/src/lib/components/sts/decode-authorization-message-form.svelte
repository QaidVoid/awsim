<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import ScanIcon from '@lucide/svelte/icons/scan-line';
	import { toast } from 'svelte-sonner';
	import { decodeAuthorizationMessage } from '$lib/api/sts';

	let encoded = $state('');
	let decoded = $state<string | null>(null);
	let working = $state(false);

	async function submit() {
		if (!encoded.trim()) {
			toast.error('Paste an encoded authorization message.');
			return;
		}
		working = true;
		decoded = null;
		try {
			decoded = await decodeAuthorizationMessage(encoded.trim());
			toast.success('Decoded.');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Decode failed');
		} finally {
			working = false;
		}
	}

	function pretty(json: string): string {
		try {
			return JSON.stringify(JSON.parse(json), null, 2);
		} catch {
			return json;
		}
	}
</script>

<section class="flex flex-col gap-3 rounded-md border border-border bg-card/40 p-4">
	<header class="flex items-center gap-2">
		<ScanIcon class="size-4 text-muted-foreground" />
		<h2 class="text-sm font-semibold">Decode authorization message</h2>
	</header>

	<div class="flex flex-col gap-1">
		<Label for="sts-decode-input">Encoded message</Label>
		<Textarea
			id="sts-decode-input"
			bind:value={encoded}
			rows={4}
			class="font-mono text-xs"
			placeholder="Paste the encoded message returned in the AccessDenied response…"
		/>
	</div>

	<div class="flex justify-end">
		<Button onclick={submit} disabled={working || !encoded.trim()}>
			{working ? 'Decoding…' : 'Decode'}
		</Button>
	</div>

	{#if decoded}
		<pre
			class="max-h-80 overflow-auto rounded-md border border-border bg-muted/40 p-3 font-mono text-[11px] whitespace-pre-wrap break-all">{pretty(
				decoded
			)}</pre>
	{/if}
</section>
