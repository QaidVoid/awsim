<script lang="ts">
	import { onMount } from 'svelte';
	import {
		getBedrockProxyConfig,
		checkBedrockBackend,
		type BedrockProxyConfig,
		type BedrockBackendCheckResult,
	} from '$lib/api/bedrock';
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
	import CheckCircle2Icon from '@lucide/svelte/icons/check-circle-2';
	import XCircleIcon from '@lucide/svelte/icons/x-circle';
	import InfoIcon from '@lucide/svelte/icons/info';
	import ZapIcon from '@lucide/svelte/icons/zap';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import { toast } from 'svelte-sonner';
	import { route } from '$lib/url';

	let cfg = $state<BedrockProxyConfig | null>(null);
	let loading = $state(true);
	let invokeOpen = $state(false);
	let embedOpen = $state(false);
	let checks = $state<Record<string, BedrockBackendCheckResult>>({});
	let checking = $state<Record<string, boolean>>({});

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

	async function runCheck(name: string) {
		checking[name] = true;
		try {
			checks[name] = await checkBedrockBackend(name);
		} catch (e) {
			checks[name] = { ok: false, error: e instanceof Error ? e.message : 'check failed' };
		} finally {
			checking[name] = false;
		}
	}

	// Aggregate required model tags per backend, walking both invoke
	// and embed mappings. Bare-tag entries fall through to the default
	// backend, so attribute them there. Used to render the "models
	// needed on <backend>" hint and to highlight missing installs.
	let requiredByBackend = $derived.by(() => {
		const out: Record<string, Set<string>> = {};
		if (!cfg) return out;
		const fallback = cfg.defaultBackend;
		const add = (b: string | null, tag: string) => {
			const target = b ?? fallback;
			if (!target) return;
			if (!out[target]) out[target] = new Set();
			out[target].add(tag);
		};
		for (const e of cfg.invoke) add(e.backend, e.tag);
		for (const e of cfg.embed) add(e.backend, e.tag);
		return out;
	});

	function pullCommand(backendName: string, endpoint: string, tags: string[]): string | null {
		// Heuristic: assume Ollama for `localhost:11434` (the default
		// Ollama port) since `ollama pull` is the install command users
		// most commonly need. Other servers don't have a one-liner
		// install path so we don't fake one.
		if (endpoint.includes(':11434')) {
			return `ollama pull ${tags.join(' ')}`;
		}
		void backendName;
		return null;
	}

	function copyToClipboard(text: string) {
		navigator.clipboard
			.writeText(text)
			.then(() => toast.success('Copied'))
			.catch(() => toast.error('Copy failed'));
	}

	function missingTags(backendName: string): string[] {
		const required = requiredByBackend[backendName];
		const result = checks[backendName];
		if (!required || !result?.ok || !result.models) return [];
		const installed = new Set(result.models);
		return [...required].filter((t) => !installed.has(t));
	}
</script>

<div class="space-y-6 p-4">
	<!-- Always-visible explainer. The proxy is opaque enough that
	     users land here confused about what's happening — front-load
	     the mental model before showing config. -->
	<section class="rounded-lg border bg-card">
		<div class="flex gap-3 p-4">
			<InfoIcon class="mt-0.5 h-5 w-5 shrink-0 text-muted-foreground" />
			<div class="space-y-2 text-sm">
				<h3 class="font-semibold">How the Bedrock proxy works</h3>
				<p class="text-muted-foreground">
					Every Bedrock <code>InvokeModel</code> / <code>Converse</code> /
					<code>InvokeModel</code> embedding call is translated into an
					OpenAI-compatible <code>chat.completions</code> or <code>embeddings</code> request and
					forwarded to a backend you configure. The translator handles
					Anthropic, Titan, Llama, Mistral, Cohere, and the embedding families.
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
				<p class="text-muted-foreground">
					Each Bedrock model id is mapped to a backend-side model tag below.
					Edit backends and mappings on the <a class="underline" href={route('/settings')}>Settings page</a>.
				</p>
			</div>
		</div>
	</section>

	<header class="flex items-center justify-between">
		<div>
			<h2 class="text-lg font-semibold">Proxy backends</h2>
			<p class="text-sm text-muted-foreground">
				API keys are not displayed for security.
			</p>
		</div>
		<div class="flex gap-2">
			<Button variant="outline" size="sm" href={route('/settings')}>
				<SettingsIcon class="h-4 w-4" />
				<span class="ml-2">Edit</span>
			</Button>
			<Button variant="ghost" size="sm" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'h-4 w-4 animate-spin' : 'h-4 w-4'} />
				<span class="ml-2">Refresh</span>
			</Button>
		</div>
	</header>

	{#if loading && !cfg}
		<EmptyState icon={ServerIcon} title="Loading…" description="Fetching backend config." />
	{:else if cfg && !cfg.enabled}
		<EmptyState
			icon={CircleAlertIcon}
			title="Canned-response mode"
			description="No Bedrock proxy backend is configured. InvokeModel / Converse return deterministic canned responses. Open Settings to wire up a real LLM."
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
							<TableHead class="text-right">Health</TableHead>
						</TableRow>
					</TableHeader>
					<TableBody>
						{#each cfg.backends as b (b.name)}
							{@const result = checks[b.name]}
							{@const missing = missingTags(b.name)}
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
								<TableCell class="text-right">
									<Button
										variant="ghost"
										size="sm"
										onclick={() => runCheck(b.name)}
										disabled={checking[b.name]}
									>
										<ZapIcon class={checking[b.name] ? 'h-4 w-4 animate-pulse' : 'h-4 w-4'} />
										<span class="ml-2">Check</span>
									</Button>
								</TableCell>
							</TableRow>
							{#if result}
								<TableRow>
									<TableCell colspan={5} class="bg-muted/30">
										{#if result.ok}
											<div class="space-y-2 text-sm">
												<div class="flex items-center gap-2">
													<CheckCircle2Icon class="h-4 w-4 text-green-500" />
													<span class="font-medium">Reachable</span>
													{#if result.latencyMs !== undefined}
														<span class="text-xs text-muted-foreground">
															{result.latencyMs}ms
														</span>
													{/if}
													{#if result.models}
														<span class="text-xs text-muted-foreground">
															· {result.models.length} model{result.models.length === 1 ? '' : 's'} installed
														</span>
													{/if}
												</div>
												{#if result.warning}
													<p class="text-xs text-amber-600">{result.warning}</p>
												{/if}
												{#if missing.length > 0}
													{@const cmd = pullCommand(b.name, b.endpoint, missing)}
													<div class="rounded border border-amber-500/30 bg-amber-500/5 p-2">
														<p class="text-xs font-medium text-amber-600">
															Missing on backend ({missing.length}):
														</p>
														<div class="mt-1 flex flex-wrap gap-1">
															{#each missing as tag (tag)}
																<Badge variant="outline" class="font-mono text-xs">{tag}</Badge>
															{/each}
														</div>
														{#if cmd}
															<div class="mt-2 flex items-center gap-2">
																<code class="rounded bg-background px-2 py-1 text-xs">{cmd}</code>
																<Button variant="ghost" size="icon" onclick={() => copyToClipboard(cmd)}>
																	<CopyIcon class="h-3 w-3" />
																</Button>
															</div>
														{/if}
													</div>
												{/if}
												{#if result.models && result.models.length > 0}
													<details class="text-xs">
														<summary class="cursor-pointer text-muted-foreground hover:text-foreground">
															Installed models ({result.models.length})
														</summary>
														<div class="mt-1 flex flex-wrap gap-1">
															{#each result.models as m (m)}
																<Badge variant="secondary" class="font-mono text-xs">{m}</Badge>
															{/each}
														</div>
													</details>
												{/if}
											</div>
										{:else}
											<div class="space-y-1 text-sm">
												<div class="flex items-center gap-2">
													<XCircleIcon class="h-4 w-4 text-red-500" />
													<span class="font-medium">Unreachable</span>
												</div>
												<p class="text-xs text-muted-foreground">{result.error}</p>
											</div>
										{/if}
									</TableCell>
								</TableRow>
							{/if}
						{/each}
					</TableBody>
				</Table>
			</div>
		</section>

		<!-- Required models hint. Always shown when backends + mappings
		     are present so users know what to install before runtime. -->
		{#if Object.keys(requiredByBackend).length > 0}
			<section class="rounded-lg border bg-card">
				<header class="border-b p-4">
					<h3 class="text-sm font-semibold">Required models</h3>
					<p class="mt-1 text-xs text-muted-foreground">
						Tags referenced by the active mappings, grouped by the backend that serves them.
						Run a Check above to see which are actually installed.
					</p>
				</header>
				<div class="space-y-3 p-4">
					{#each Object.entries(requiredByBackend) as [name, tags] (name)}
						{@const tagList = [...tags].sort()}
						{@const endpoint = cfg.backends.find((b) => b.name === name)?.endpoint ?? ''}
						{@const cmd = pullCommand(name, endpoint, tagList)}
						<div>
							<div class="flex items-center gap-2">
								<Badge variant="secondary" class="font-mono">{name}</Badge>
								<span class="text-xs text-muted-foreground">
									{tagList.length} tag{tagList.length === 1 ? '' : 's'}
								</span>
							</div>
							<div class="mt-2 flex flex-wrap gap-1">
								{#each tagList as t (t)}
									<Badge variant="outline" class="font-mono text-xs">{t}</Badge>
								{/each}
							</div>
							{#if cmd}
								<div class="mt-2 flex items-center gap-2">
									<code class="rounded bg-muted px-2 py-1 text-xs">{cmd}</code>
									<Button variant="ghost" size="icon" onclick={() => copyToClipboard(cmd)}>
										<CopyIcon class="h-3 w-3" />
									</Button>
								</div>
							{/if}
						</div>
					{/each}
				</div>
			</section>
		{/if}

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
