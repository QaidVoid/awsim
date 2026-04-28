<script lang="ts">
	/**
	 * Global "Inspect" drawer. Loads the captured headers + bodies for any
	 * recent request via `/_awsim/requests/{id}` and renders them in a
	 * tabbed view (Overview / Request / Response / curl).
	 *
	 * Driven by `inspectState` so any component (request stream, request
	 * log, hotkey, palette) can pop it open with just an event id.
	 */
	import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '$lib/components/ui/sheet';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { Separator } from '$lib/components/ui/separator';
	import { Tooltip, TooltipContent, TooltipTrigger } from '$lib/components/ui/tooltip';
	import { toast } from 'svelte-sonner';
	import Copy from '@lucide/svelte/icons/copy';
	import AlertCircle from '@lucide/svelte/icons/alert-circle';
	import Repeat from '@lucide/svelte/icons/repeat';
	import { decodeBody, toCurl } from '$lib/body-decode';
	import { bytesHuman, relativeTime } from '$lib/format';
	import type { RequestDetail, RequestEvent, CapturedHeader } from '$lib/events';
	import { inspectState } from '$lib/inspect-state.svelte';

	let detail = $state<RequestDetail | null>(null);
	let loading = $state(false);
	let loadError = $state<string | null>(null);
	let activeTab = $state<'request' | 'response' | 'curl'>('request');
	let replaying = $state(false);

	$effect(() => {
		if (!inspectState.open || !inspectState.eventId) return;
		loadDetail(inspectState.eventId);
	});

	async function loadDetail(id: string) {
		loading = true;
		loadError = null;
		detail = null;
		activeTab = 'request';
		try {
			const res = await fetch(`/_awsim/requests/${id}`);
			if (res.status === 404) {
				loadError = 'This request has rolled out of the in-memory ring buffer.';
				return;
			}
			if (!res.ok) {
				loadError = `Failed to load request (${res.status})`;
				return;
			}
			detail = (await res.json()) as RequestDetail;
		} catch (err) {
			loadError = err instanceof Error ? err.message : 'Failed to load request';
		} finally {
			loading = false;
		}
	}

	const event = $derived<RequestEvent | null>(inspectState.event);

	const reqDecoded = $derived(detail ? decodeBody(detail.request_body, detail.request_headers) : null);
	const resDecoded = $derived(
		detail ? decodeBody(detail.response_body, detail.response_headers) : null,
	);
	const reqUrl = $derived.by(() => {
		if (!detail) return '';
		return `${detail.path}${detail.query ? `?${detail.query}` : ''}`;
	});
	const curl = $derived.by(() => {
		if (!detail) return '';
		const base = window.location.origin;
		return toCurl(detail.method, `${base}${reqUrl}`, detail.request_headers, detail.request_body);
	});

	function statusVariant(code: number): 'default' | 'destructive' | 'outline' {
		if (code >= 500) return 'destructive';
		if (code >= 400) return 'outline';
		return 'default';
	}

	async function copy(text: string, label: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast.success(`${label} copied`);
		} catch {
			toast.error('Copy failed');
		}
	}

	async function replay() {
		if (!detail || replaying) return;
		replaying = true;
		try {
			const res = await fetch(`/_awsim/requests/${detail.id}/replay`, { method: 'POST' });
			const body = (await res.json()) as { new_id?: string; status_code?: number; error?: string; message?: string };
			if (!res.ok || !body.new_id) {
				toast.error(body.message ?? body.error ?? `Replay failed (${res.status})`);
				return;
			}
			toast.success(`Replayed → ${body.status_code}`);
			// Swap to the freshly captured detail. Clear the cached event
			// so the metadata grid doesn't show stale duration/region.
			inspectState.show(body.new_id, null);
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Replay failed');
		} finally {
			replaying = false;
		}
	}

	function headersToText(headers: CapturedHeader[]): string {
		return headers.map((h) => `${h.name}: ${h.value}`).join('\n');
	}
</script>

<Sheet
	open={inspectState.open}
	onOpenChange={(o) => {
		if (!o) inspectState.close();
	}}
>
	<SheetContent side="right" class="w-full overflow-y-auto sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle class="flex items-center gap-2">
				<Badge variant="outline" class="font-mono">
					{detail?.method ?? event?.method ?? '—'}
				</Badge>
				<span class="flex-1 truncate font-mono text-xs text-muted-foreground">
					{detail ? reqUrl : (event?.path ?? '…')}
				</span>
				{#if detail}
					{#if detail.request_body.truncated}
						<Tooltip>
							<TooltipTrigger>
								{#snippet child({ props })}
									<Button {...props} type="button" variant="outline" size="sm" disabled class="h-7 gap-1 px-2">
										<Repeat class="size-3.5" />
										<span class="text-xs">Replay</span>
									</Button>
								{/snippet}
							</TooltipTrigger>
							<TooltipContent>
								<p class="text-xs">Body was truncated — replay would not be faithful.</p>
							</TooltipContent>
						</Tooltip>
					{:else}
						<Button
							type="button"
							variant="outline"
							size="sm"
							onclick={replay}
							disabled={replaying}
							class="h-7 gap-1 px-2"
						>
							<Repeat class={`size-3.5 ${replaying ? 'animate-spin' : ''}`} />
							<span class="text-xs">{replaying ? 'Replaying…' : 'Replay'}</span>
						</Button>
					{/if}
				{/if}
			</SheetTitle>
			<SheetDescription>
				{event?.service ?? 'Request inspector'}
				{#if event?.operation}· {event.operation}{/if}
				{#if event}· {relativeTime(event.ts)}{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="space-y-4 px-4 pb-6">
			{#if loadError}
				<div class="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-xs">
					<AlertCircle class="mt-0.5 size-4 shrink-0 text-destructive" />
					<div>
						<p class="font-medium text-destructive">Detail unavailable</p>
						<p class="mt-0.5 text-muted-foreground">{loadError}</p>
					</div>
				</div>
			{:else if loading && !detail}
				<div class="space-y-3">
					<Skeleton class="h-20 w-full" />
					<Skeleton class="h-40 w-full" />
				</div>
			{:else if detail}
				<dl class="grid grid-cols-2 gap-x-4 gap-y-3 text-xs">
					<div>
						<dt class="text-muted-foreground">Status</dt>
						<dd class="mt-1">
							<Badge variant={statusVariant(detail.status_code)} class="font-mono">
								{detail.status_code}
							</Badge>
						</dd>
					</div>
					{#if event}
						<div>
							<dt class="text-muted-foreground">Duration</dt>
							<dd class="mt-1 font-mono">{event.duration_ms.toFixed(1)} ms</dd>
						</div>
						<div>
							<dt class="text-muted-foreground">Region</dt>
							<dd class="mt-1 font-mono">{event.region}</dd>
						</div>
						<div>
							<dt class="text-muted-foreground">Account</dt>
							<dd class="mt-1 font-mono">{event.account_id}</dd>
						</div>
					{/if}
					<div>
						<dt class="text-muted-foreground">Request size</dt>
						<dd class="mt-1 font-mono">{bytesHuman(detail.request_body.size)}</dd>
					</div>
					<div>
						<dt class="text-muted-foreground">Response size</dt>
						<dd class="mt-1 font-mono">{bytesHuman(detail.response_body.size)}</dd>
					</div>
					{#if event?.principal_arn}
						<div class="col-span-2">
							<dt class="text-muted-foreground">Principal</dt>
							<dd class="mt-1 truncate font-mono text-[11px]">{event.principal_arn}</dd>
						</div>
					{/if}
					{#if event?.error_code}
						<div class="col-span-2">
							<dt class="text-muted-foreground">Error</dt>
							<dd class="mt-1 font-mono text-destructive">{event.error_code}</dd>
						</div>
					{/if}
				</dl>

				<Separator />

				<Tabs bind:value={activeTab}>
					<TabsList class="grid w-full grid-cols-3">
						<TabsTrigger value="request">Request</TabsTrigger>
						<TabsTrigger value="response">Response</TabsTrigger>
						<TabsTrigger value="curl">curl</TabsTrigger>
					</TabsList>

					<TabsContent value="request" class="space-y-3 pt-3">
						<section>
							<header class="mb-1.5 flex items-center justify-between">
								<h3 class="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
									Headers ({detail.request_headers.length})
								</h3>
								<Button
									variant="ghost"
									size="sm"
									class="h-6 gap-1 px-2"
									onclick={() => copy(headersToText(detail!.request_headers), 'Headers')}
								>
									<Copy class="size-3" /> Copy
								</Button>
							</header>
							<pre class="overflow-x-auto rounded-md border border-border bg-muted/40 p-2 font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-all">{headersToText(detail.request_headers)}</pre>
						</section>

						<section>
							<header class="mb-1.5 flex items-center justify-between">
								<h3 class="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
									Body
									{#if reqDecoded}
										<span class="font-mono text-[10px] normal-case text-muted-foreground/80">
											{reqDecoded.kind} · {bytesHuman(reqDecoded.size)}
											{#if reqDecoded.truncated}· truncated{/if}
										</span>
									{/if}
								</h3>
								{#if reqDecoded && reqDecoded.kind !== 'empty'}
									<Button
										variant="ghost"
										size="sm"
										class="h-6 gap-1 px-2"
										onclick={() => copy(reqDecoded!.text, 'Body')}
									>
										<Copy class="size-3" /> Copy
									</Button>
								{/if}
							</header>
							{#if !reqDecoded || reqDecoded.kind === 'empty'}
								<p class="rounded-md border border-dashed border-border bg-muted/20 p-3 text-center text-[11px] text-muted-foreground">
									No request body.
								</p>
							{:else}
								<pre class="max-h-[40vh] overflow-auto rounded-md border border-border bg-muted/40 p-2 font-mono text-[11px] leading-relaxed">{reqDecoded.text}</pre>
							{/if}
						</section>
					</TabsContent>

					<TabsContent value="response" class="space-y-3 pt-3">
						<section>
							<header class="mb-1.5 flex items-center justify-between">
								<h3 class="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
									Headers ({detail.response_headers.length})
								</h3>
								<Button
									variant="ghost"
									size="sm"
									class="h-6 gap-1 px-2"
									onclick={() => copy(headersToText(detail!.response_headers), 'Headers')}
								>
									<Copy class="size-3" /> Copy
								</Button>
							</header>
							<pre class="overflow-x-auto rounded-md border border-border bg-muted/40 p-2 font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-all">{headersToText(detail.response_headers)}</pre>
						</section>

						<section>
							<header class="mb-1.5 flex items-center justify-between">
								<h3 class="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
									Body
									{#if resDecoded}
										<span class="font-mono text-[10px] normal-case text-muted-foreground/80">
											{resDecoded.kind} · {bytesHuman(resDecoded.size)}
											{#if resDecoded.truncated}· truncated{/if}
										</span>
									{/if}
								</h3>
								{#if resDecoded && resDecoded.kind !== 'empty'}
									<Button
										variant="ghost"
										size="sm"
										class="h-6 gap-1 px-2"
										onclick={() => copy(resDecoded!.text, 'Body')}
									>
										<Copy class="size-3" /> Copy
									</Button>
								{/if}
							</header>
							{#if !resDecoded || resDecoded.kind === 'empty'}
								<p class="rounded-md border border-dashed border-border bg-muted/20 p-3 text-center text-[11px] text-muted-foreground">
									No response body.
								</p>
							{:else}
								<pre class="max-h-[40vh] overflow-auto rounded-md border border-border bg-muted/40 p-2 font-mono text-[11px] leading-relaxed">{resDecoded.text}</pre>
							{/if}
						</section>
					</TabsContent>

					<TabsContent value="curl" class="space-y-3 pt-3">
						<header class="flex items-center justify-between">
							<h3 class="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
								Reproduce as curl
							</h3>
							<Button
								variant="ghost"
								size="sm"
								class="h-6 gap-1 px-2"
								onclick={() => copy(curl, 'curl')}
							>
								<Copy class="size-3" /> Copy
							</Button>
						</header>
						<pre class="max-h-[60vh] overflow-auto rounded-md border border-border bg-muted/40 p-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap">{curl}</pre>
					</TabsContent>
				</Tabs>
			{/if}
		</div>
	</SheetContent>
</Sheet>
