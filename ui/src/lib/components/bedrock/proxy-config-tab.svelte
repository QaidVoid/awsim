<script lang="ts">
	import { onMount } from 'svelte';
	import { getBedrockProxyConfig, type BedrockProxyConfig } from '$lib/api/bedrock';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { EmptyState } from '$lib/components/service';
	import {
		Table,
		TableBody,
		TableCell,
		TableHead,
		TableHeader,
		TableRow,
	} from '$lib/components/ui/table';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import ServerIcon from '@lucide/svelte/icons/server';
	import KeyIcon from '@lucide/svelte/icons/key';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { toast } from 'svelte-sonner';

	let cfg = $state<BedrockProxyConfig | null>(null);
	let loading = $state(true);
	let invokeOpen = $state(false);
	let embedOpen = $state(false);

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

<div class="space-y-6 p-4">
	<header class="flex items-center justify-between">
		<div>
			<h2 class="text-lg font-semibold">Proxy backends</h2>
			<p class="text-sm text-muted-foreground">
				OpenAI-compatible endpoints handling Bedrock InvokeModel / Converse / embeddings.
				API keys are not displayed for security.
			</p>
		</div>
		<Button variant="ghost" size="sm" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'h-4 w-4 animate-spin' : 'h-4 w-4'} />
			<span class="ml-2">Refresh</span>
		</Button>
	</header>

	{#if loading && !cfg}
		<EmptyState icon={ServerIcon} title="Loading…" description="Fetching backend config." />
	{:else if cfg && !cfg.enabled}
		<EmptyState
			icon={CircleAlertIcon}
			title="Canned-response mode"
			description="No Bedrock proxy backend is configured. InvokeModel / Converse return deterministic canned responses. Set --bedrock-backend or --bedrock-config to wire up a real LLM."
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
			<div class="border-t">
				<Table>
					<TableHeader>
						<TableRow>
							<TableHead>Name</TableHead>
							<TableHead>Endpoint</TableHead>
							<TableHead>Auth</TableHead>
							<TableHead>Default</TableHead>
						</TableRow>
					</TableHeader>
					<TableBody>
						{#each cfg.backends as b (b.name)}
							<TableRow>
								<TableCell class="font-mono text-sm">{b.name}</TableCell>
								<TableCell class="font-mono text-xs text-muted-foreground">{b.endpoint}</TableCell>
								<TableCell>
									{#if b.hasApiKey}
										<Badge variant="secondary" class="gap-1">
											<KeyIcon class="h-3 w-3" /> bearer
										</Badge>
									{:else}
										<Badge variant="outline">none</Badge>
									{/if}
								</TableCell>
								<TableCell>
									{#if b.name === cfg.defaultBackend}
										<Badge variant="default">default</Badge>
									{:else}
										<span class="text-muted-foreground">—</span>
									{/if}
								</TableCell>
							</TableRow>
						{/each}
					</TableBody>
				</Table>
			</div>
		</section>

		<section class="rounded-lg border bg-card">
			<button
				type="button"
				class="flex w-full items-center justify-between p-4 text-left"
				onclick={() => (invokeOpen = !invokeOpen)}
			>
				<div>
					<h3 class="text-sm font-semibold">Invoke / Converse mappings</h3>
					<p class="text-xs text-muted-foreground">
						Bedrock model id → backend tag. Routed entries pin a specific backend; the rest fall through to the default.
					</p>
				</div>
				<ChevronRightIcon
					class={invokeOpen ? 'h-4 w-4 rotate-90 transition-transform' : 'h-4 w-4 transition-transform'}
				/>
			</button>
			{#if invokeOpen}
				<div class="border-t">
					<Table>
						<TableHeader>
							<TableRow>
								<TableHead>Bedrock id</TableHead>
								<TableHead>Backend</TableHead>
								<TableHead>Tag</TableHead>
							</TableRow>
						</TableHeader>
						<TableBody>
							{#each cfg.invoke as e (e.id)}
								<TableRow>
									<TableCell class="font-mono text-xs">{e.id}</TableCell>
									<TableCell>
										{#if e.backend}
											<Badge variant="secondary" class="font-mono text-xs">{e.backend}</Badge>
										{:else}
											<span class="text-xs text-muted-foreground">{cfg.defaultBackend ?? '—'} (default)</span>
										{/if}
									</TableCell>
									<TableCell class="font-mono text-xs">{e.tag}</TableCell>
								</TableRow>
							{/each}
						</TableBody>
					</Table>
				</div>
			{/if}
		</section>

		<section class="rounded-lg border bg-card">
			<button
				type="button"
				class="flex w-full items-center justify-between p-4 text-left"
				onclick={() => (embedOpen = !embedOpen)}
			>
				<div>
					<h3 class="text-sm font-semibold">Embedding mappings</h3>
					<p class="text-xs text-muted-foreground">
						Embedding-only Bedrock ids (Titan Embed, Cohere Embed) → backend tag.
					</p>
				</div>
				<ChevronRightIcon
					class={embedOpen ? 'h-4 w-4 rotate-90 transition-transform' : 'h-4 w-4 transition-transform'}
				/>
			</button>
			{#if embedOpen}
				<div class="border-t">
					<Table>
						<TableHeader>
							<TableRow>
								<TableHead>Bedrock id</TableHead>
								<TableHead>Backend</TableHead>
								<TableHead>Tag</TableHead>
							</TableRow>
						</TableHeader>
						<TableBody>
							{#each cfg.embed as e (e.id)}
								<TableRow>
									<TableCell class="font-mono text-xs">{e.id}</TableCell>
									<TableCell>
										{#if e.backend}
											<Badge variant="secondary" class="font-mono text-xs">{e.backend}</Badge>
										{:else}
											<span class="text-xs text-muted-foreground">{cfg.defaultBackend ?? '—'} (default)</span>
										{/if}
									</TableCell>
									<TableCell class="font-mono text-xs">{e.tag}</TableCell>
								</TableRow>
							{/each}
						</TableBody>
					</Table>
				</div>
			{/if}
		</section>
	{/if}
</div>
