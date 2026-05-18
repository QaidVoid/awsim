<script lang="ts">
	/**
	 * Playground — low-level AWS request builder. Pick a template,
	 * tweak headers/body, hit Send. Useful for poking AWSim without
	 * needing an SDK or the AWS CLI installed.
	 *
	 * Requests are POSTed straight to the gateway at the same origin,
	 * so they flow through the same ServiceHandler dispatch as a real
	 * SDK call (auth, IAM enforcement, chaos rules, billing meter).
	 */
	import { ServicePage } from '$lib/components/service';
	import { fetchRecentRequestIds } from '$lib/api/requests';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Separator } from '$lib/components/ui/separator';
	import PlayIcon from '@lucide/svelte/icons/play';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import EyeIcon from '@lucide/svelte/icons/eye';
	import { toast } from 'svelte-sonner';
	import { TEMPLATES, type RequestTemplate } from './templates';
	import { inspectState } from '$lib/inspect-state.svelte';

	let method = $state<RequestTemplate['method']>('GET');
	let path = $state('/');
	let headers = $state<{ key: string; value: string }[]>([]);
	let body = $state('');
	let templateId = $state<string>('');

	let sending = $state(false);
	let response = $state<{
		status: number;
		statusText: string;
		headers: { key: string; value: string }[];
		body: string;
		duration_ms: number;
		new_id?: string | null;
	} | null>(null);

	function applyTemplate(t: RequestTemplate) {
		templateId = t.id;
		method = t.method;
		path = t.path;
		headers = t.headers.map((h) => ({ ...h }));
		body = t.body;
		response = null;
	}

	function addHeader() {
		headers = [...headers, { key: '', value: '' }];
	}

	function removeHeader(i: number) {
		headers = headers.filter((_, idx) => idx !== i);
	}

	async function send() {
		if (sending) return;
		sending = true;
		const start = performance.now();
		try {
			const cleaned = headers.filter((h) => h.key.trim() !== '');
			const headersInit: HeadersInit = Object.fromEntries(
				cleaned.map((h) => [h.key, h.value]),
			);

			const init: RequestInit = {
				method,
				headers: headersInit,
			};
			if (method !== 'GET' && method !== 'HEAD') {
				init.body = body;
			}

			const res = await fetch(path, init);
			const duration_ms = performance.now() - start;
			const respHeaders: { key: string; value: string }[] = [];
			res.headers.forEach((value, key) => respHeaders.push({ key, value }));

			let respBody = await res.text();
			// Pretty-print JSON responses.
			const ct = res.headers.get('content-type') ?? '';
			if (ct.includes('json')) {
				try {
					respBody = JSON.stringify(JSON.parse(respBody), null, 2);
				} catch {
					// Leave as-is if it's not valid JSON.
				}
			}

			// The gateway doesn't echo the captured request id on the
			// response, so look up the freshest captured id from the
			// admin endpoint. Safe in practice because a user editing
			// in the playground generates serial requests.
			let newId: string | null = null;
			try {
				newId = (await fetchRecentRequestIds())[0] ?? null;
			} catch {
				// Best-effort — Inspect button just stays hidden.
			}

			response = {
				status: res.status,
				statusText: res.statusText,
				headers: respHeaders,
				body: respBody,
				duration_ms,
				new_id: newId,
			};
			if (res.ok) {
				toast.success(`${res.status} ${res.statusText} · ${duration_ms.toFixed(0)}ms`);
			} else {
				toast.error(`${res.status} ${res.statusText}`);
			}
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Request failed');
			response = {
				status: 0,
				statusText: 'Network error',
				headers: [],
				body: err instanceof Error ? err.message : String(err),
				duration_ms: performance.now() - start,
			};
		} finally {
			sending = false;
		}
	}

	function inspectResponse() {
		if (response?.new_id) {
			inspectState.show(response.new_id, null);
		}
	}

	async function copy(text: string, label: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast.success(`${label} copied`);
		} catch {
			toast.error('Copy failed');
		}
	}

	function statusVariant(code: number): 'default' | 'destructive' | 'outline' {
		if (code === 0) return 'destructive';
		if (code >= 500) return 'destructive';
		if (code >= 400) return 'outline';
		return 'default';
	}
</script>

<svelte:head>
	<title>AWSim · Playground</title>
</svelte:head>

<ServicePage
	title="Request playground"
	description="Low-level AWS request builder. Pick a template, edit, send. Requests flow through the same gateway as real SDK calls — auth, IAM, chaos rules, and billing all apply."
>
	<div class="space-y-6 p-6">
		<!-- Templates -->
		<section>
			<h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
				Templates
			</h2>
			<div class="flex flex-wrap gap-2">
				{#each TEMPLATES as t (t.id)}
					<Button
						type="button"
						variant={templateId === t.id ? 'default' : 'outline'}
						size="sm"
						onclick={() => applyTemplate(t)}
						class="h-7 px-2 text-xs"
					>
						{t.label}
					</Button>
				{/each}
			</div>
		</section>

		<!-- Request -->
		<section class="space-y-3">
			<h2 class="text-sm font-semibold uppercase tracking-wide text-muted-foreground">
				Request
			</h2>
			<div class="flex items-center gap-2">
				<select
					bind:value={method}
					class="h-9 rounded border border-border bg-background px-2 font-mono text-xs"
				>
					<option>GET</option>
					<option>POST</option>
					<option>PUT</option>
					<option>DELETE</option>
					<option>HEAD</option>
					<option>PATCH</option>
				</select>
				<Input
					bind:value={path}
					placeholder="/"
					class="flex-1 font-mono text-xs"
					spellcheck={false}
				/>
				<Button onclick={send} disabled={sending}>
					<PlayIcon class={`mr-1 h-4 w-4 ${sending ? 'animate-pulse' : ''}`} />
					{sending ? 'Sending…' : 'Send'}
				</Button>
			</div>

			<div>
				<div class="mb-1 flex items-center justify-between">
					<Label>Headers</Label>
					<Button variant="ghost" size="sm" onclick={addHeader} class="h-7 gap-1 px-2">
						<PlusIcon class="h-3.5 w-3.5" /> Add
					</Button>
				</div>
				{#if headers.length === 0}
					<p class="rounded border border-dashed border-border p-3 text-center text-xs text-muted-foreground">
						No headers. Click Add or pick a template.
					</p>
				{:else}
					<div class="space-y-1">
						{#each headers as h, i (i)}
							<div class="flex items-center gap-1">
								<Input
									bind:value={h.key}
									placeholder="Header name"
									class="flex-1 font-mono text-xs"
									spellcheck={false}
								/>
								<Input
									bind:value={h.value}
									placeholder="value"
									class="flex-[2] font-mono text-xs"
									spellcheck={false}
								/>
								<Button
									variant="ghost"
									size="sm"
									onclick={() => removeHeader(i)}
									class="h-9 px-2"
								>
									<Trash2Icon class="h-4 w-4" />
								</Button>
							</div>
						{/each}
					</div>
				{/if}
			</div>

			<div>
				<Label for="req-body">Body</Label>
				<Textarea
					id="req-body"
					bind:value={body}
					placeholder="Request body (JSON, query string, …)"
					class="mt-1 min-h-[140px] font-mono text-xs"
					spellcheck={false}
				/>
			</div>
		</section>

		{#if response}
			<Separator />
			<!-- Response -->
			<section class="space-y-3">
				<div class="flex items-center justify-between">
					<h2 class="text-sm font-semibold uppercase tracking-wide text-muted-foreground">
						Response
					</h2>
					<div class="flex items-center gap-2">
						<Badge variant={statusVariant(response.status)} class="font-mono">
							{response.status} {response.statusText}
						</Badge>
						<span class="font-mono text-xs text-muted-foreground">
							{response.duration_ms.toFixed(0)}ms
						</span>
						{#if response.new_id}
							<Button
								variant="ghost"
								size="sm"
								onclick={inspectResponse}
								class="h-7 gap-1 px-2"
							>
								<EyeIcon class="h-3.5 w-3.5" /> Inspect
							</Button>
						{/if}
						<Button
							variant="ghost"
							size="sm"
							onclick={() => copy(response!.body, 'Body')}
							class="h-7 gap-1 px-2"
						>
							<CopyIcon class="h-3.5 w-3.5" /> Copy
						</Button>
					</div>
				</div>

				{#if response.headers.length > 0}
					<details class="rounded border border-border">
						<summary class="cursor-pointer px-3 py-2 text-xs text-muted-foreground hover:bg-muted/40">
							Headers ({response.headers.length})
						</summary>
						<pre class="border-t border-border bg-muted/30 p-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-all">{response.headers
								.map((h) => `${h.key}: ${h.value}`)
								.join('\n')}</pre>
					</details>
				{/if}

				<pre class="max-h-[60vh] overflow-auto rounded border border-border bg-muted/40 p-3 font-mono text-[11px] leading-relaxed">{response.body || '(empty)'}</pre>
			</section>
		{/if}
	</div>
</ServicePage>
