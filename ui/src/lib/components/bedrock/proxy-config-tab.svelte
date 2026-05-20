<script lang="ts">
	// Thin deferral card. The Model Gateway page (/gateway) owns the
	// full editor + health board + activity tab; this tab is just a
	// signpost that surfaces canned-vs-real mode and a button to open
	// the gateway. See the gateway page for backends, credentials,
	// model aliases, routing, and health.
	import { onMount } from 'svelte';
	import { getBedrockProxyConfig, type BedrockProxyConfig } from '$lib/api/bedrock';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import ServerIcon from '@lucide/svelte/icons/server';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import InfoIcon from '@lucide/svelte/icons/info';
	import NetworkIcon from '@lucide/svelte/icons/network';
	import { toast } from 'svelte-sonner';
	import { route } from '$lib/url';

	let cfg = $state<BedrockProxyConfig | null>(null);
	let loading = $state(true);

	onMount(load);

	async function load() {
		loading = true;
		try {
			cfg = await getBedrockProxyConfig();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load proxy config');
		} finally {
			loading = false;
		}
	}
</script>

<div class="space-y-4 p-4">
	<section class="rounded-lg border bg-card">
		<div class="flex gap-3 p-4">
			<InfoIcon class="mt-0.5 h-5 w-5 shrink-0 text-muted-foreground" />
			<div class="space-y-2 text-sm">
				<h3 class="font-semibold">How the Bedrock proxy works</h3>
				<p class="text-muted-foreground">
					Every Bedrock <code>InvokeModel</code> / <code>Converse</code> / embedding call is
					translated into an OpenAI-compatible <code>chat.completions</code> or
					<code>embeddings</code> request and forwarded to a backend you configure on the
					<a class="underline" href={route('/gateway')}>Model Gateway</a> page. The translator
					handles Anthropic, Titan, Llama, Mistral, Cohere, and the embedding families.
				</p>
				<p class="text-muted-foreground">
					Compatible backends:
					<a class="underline" href="https://ollama.com" target="_blank" rel="noopener">Ollama</a>,
					<a class="underline" href="https://lmstudio.ai" target="_blank" rel="noopener">LM Studio</a>,
					<a class="underline" href="https://github.com/vllm-project/vllm" target="_blank" rel="noopener">vLLM</a>,
					<a class="underline" href="https://github.com/ggerganov/llama.cpp" target="_blank" rel="noopener">llama.cpp</a>,
					<a class="underline" href="https://localai.io" target="_blank" rel="noopener">LocalAI</a>,
					Groq, OpenAI, or anything that speaks <code>POST /v1/chat/completions</code>.
				</p>
			</div>
		</div>
	</section>

	<header class="flex items-center justify-between">
		<div>
			<h2 class="text-lg font-semibold">Current state</h2>
			<p class="text-sm text-muted-foreground">
				A read-only snapshot of the live proxy registry. Edit on the Model Gateway page.
			</p>
		</div>
		<div class="flex gap-2">
			<Button variant="outline" size="sm" href={route('/gateway') + '?tab=backends'}>
				<NetworkIcon class="h-4 w-4" />
				<span class="ml-2">Open Model Gateway</span>
			</Button>
			<Button variant="ghost" size="sm" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'h-4 w-4 animate-spin' : 'h-4 w-4'} />
				<span class="ml-2">Refresh</span>
			</Button>
		</div>
	</header>

	{#if loading && !cfg}
		<EmptyState icon={ServerIcon} title="Loading…" description="Fetching proxy state." />
	{:else if cfg && !cfg.enabled}
		<EmptyState
			icon={CircleAlertIcon}
			title="Canned-response mode"
			description="No Bedrock proxy backend is configured. InvokeModel / Converse return deterministic canned responses. Open the Model Gateway to wire up a real LLM."
			action={openGatewayAction}
		/>
	{:else if cfg}
		<section class="rounded-lg border bg-card">
			<div class="grid grid-cols-1 gap-4 p-4 sm:grid-cols-3">
				<div>
					<div class="text-xs uppercase text-muted-foreground">Default backend</div>
					<div class="mt-1 font-mono text-sm">
						{cfg.defaultBackend ?? '—'}
					</div>
				</div>
				<div>
					<div class="text-xs uppercase text-muted-foreground">Backends</div>
					<div class="mt-1 text-sm">{cfg.backends.length}</div>
				</div>
				<div>
					<div class="text-xs uppercase text-muted-foreground">Mappings</div>
					<div class="mt-1 text-sm">
						invoke: {cfg.invoke.length} · embed: {cfg.embed.length}
					</div>
				</div>
			</div>
			<div class="flex flex-wrap gap-1 border-t p-4">
				{#each cfg.backends as b (b.name)}
					{@const isDefault = b.name === cfg.defaultBackend}
					<Badge variant={isDefault ? 'default' : 'secondary'} class="font-mono text-xs">
						{b.name}
					</Badge>
				{/each}
			</div>
		</section>
	{/if}
</div>

{#snippet openGatewayAction()}
	<Button variant="outline" size="sm" href={route('/gateway') + '?tab=backends'}>
		<NetworkIcon class="h-4 w-4" />
		<span class="ml-2">Open Model Gateway</span>
	</Button>
{/snippet}
