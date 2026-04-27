<script lang="ts">
	/**
	 * Side-drawer that shows the full payload for a single request event
	 * picked from the live stream, plus a sample `curl` reproduction
	 * users can copy/paste against the local emulator.
	 */
	import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetDescription } from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Separator } from '$lib/components/ui/separator';
	import { toast } from 'svelte-sonner';
	import Copy from '@lucide/svelte/icons/copy';
	import type { RequestEvent } from '$lib/events';
	import { bytesHuman, relativeTime } from '$lib/format';

	interface Props {
		open: boolean;
		event: RequestEvent | null;
		onOpenChange: (open: boolean) => void;
	}

	let { open, event, onOpenChange }: Props = $props();

	const curl = $derived(event ? buildCurl(event) : '');
	const json = $derived(event ? JSON.stringify(event, null, 2) : '');

	function buildCurl(evt: RequestEvent): string {
		const base = 'http://localhost:4566';
		const lines = [
			`curl -X ${evt.method} '${base}${evt.path}' \\`,
			`  -H 'Authorization: AWS4-HMAC-SHA256 Credential=test/$(date -u +%Y%m%d)/${evt.region}/${evt.service}/aws4_request, SignedHeaders=host, Signature=fake' \\`,
			"  -H 'Content-Type: application/json'",
		];
		return lines.join('\n');
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
		if (code >= 500) return 'destructive';
		if (code >= 400) return 'outline';
		return 'default';
	}
</script>

<Sheet
	{open}
	onOpenChange={onOpenChange}
>
	<SheetContent side="right" class="w-full sm:max-w-lg overflow-y-auto">
		{#if event}
			<SheetHeader>
				<SheetTitle class="flex items-center gap-2">
					<Badge variant="outline" class="font-mono">{event.method}</Badge>
					<span class="truncate font-mono text-xs text-muted-foreground">{event.path}</span>
				</SheetTitle>
				<SheetDescription>
					{event.service}
					{#if event.operation}· {event.operation}{/if}
					· {relativeTime(event.ts)}
				</SheetDescription>
			</SheetHeader>

			<div class="space-y-4 px-4 pb-6">
				<dl class="grid grid-cols-2 gap-x-4 gap-y-3 text-xs">
					<div>
						<dt class="text-muted-foreground">Status</dt>
						<dd class="mt-1">
							<Badge variant={statusVariant(event.status_code)} class="font-mono">
								{event.status_code}
							</Badge>
						</dd>
					</div>
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
					<div>
						<dt class="text-muted-foreground">Request size</dt>
						<dd class="mt-1 font-mono">{bytesHuman(event.request_size)}</dd>
					</div>
					<div>
						<dt class="text-muted-foreground">Response size</dt>
						<dd class="mt-1 font-mono">{bytesHuman(event.response_size)}</dd>
					</div>
					{#if event.principal_arn}
						<div class="col-span-2">
							<dt class="text-muted-foreground">Principal</dt>
							<dd class="mt-1 truncate font-mono text-[11px]">{event.principal_arn}</dd>
						</div>
					{/if}
					{#if event.error_code}
						<div class="col-span-2">
							<dt class="text-muted-foreground">Error</dt>
							<dd class="mt-1 font-mono text-destructive">{event.error_code}</dd>
						</div>
					{/if}
				</dl>

				<Separator />

				<section>
					<header class="mb-2 flex items-center justify-between">
						<h3 class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
							Reproduce with curl
						</h3>
						<Button variant="ghost" size="sm" onclick={() => copy(curl, 'curl')} class="h-6 gap-1 px-2">
							<Copy class="size-3" /> Copy
						</Button>
					</header>
					<pre class="overflow-x-auto rounded-md border border-border bg-muted/40 p-3 text-[11px] font-mono whitespace-pre">{curl}</pre>
				</section>

				<section>
					<header class="mb-2 flex items-center justify-between">
						<h3 class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
							Raw event
						</h3>
						<Button variant="ghost" size="sm" onclick={() => copy(json, 'JSON')} class="h-6 gap-1 px-2">
							<Copy class="size-3" /> Copy
						</Button>
					</header>
					<pre class="overflow-x-auto rounded-md border border-border bg-muted/40 p-3 text-[11px] font-mono">{json}</pre>
				</section>
			</div>
		{/if}
	</SheetContent>
</Sheet>
