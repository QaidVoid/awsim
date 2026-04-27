<script lang="ts">
	/**
	 * Side drawer that shows the full CloudTrail event payload, including
	 * the raw `cloudTrailEvent` JSON.
	 */
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle,
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Separator } from '$lib/components/ui/separator';
	import Copy from '@lucide/svelte/icons/copy';
	import { toast } from 'svelte-sonner';
	import type { TrailEvent } from '$lib/api/cloudtrail';

	interface Props {
		open: boolean;
		event: TrailEvent | null;
		onOpenChange: (open: boolean) => void;
	}

	let { open, event, onOpenChange }: Props = $props();

	const pretty = $derived(prettify(event?.cloudTrailEvent));

	function prettify(raw: string | undefined): string {
		if (!raw) return '';
		try {
			return JSON.stringify(JSON.parse(raw), null, 2);
		} catch {
			return raw;
		}
	}

	function fmtTime(ms: number): string {
		try {
			return new Date(ms).toLocaleString();
		} catch {
			return String(ms);
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
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full overflow-y-auto sm:max-w-xl">
		{#if event}
			<SheetHeader>
				<SheetTitle class="flex items-center gap-2">
					<Badge variant="outline" class="font-mono text-[10px]">{event.eventSource}</Badge>
					<span class="truncate font-mono text-xs text-muted-foreground">
						{event.eventName}
					</span>
				</SheetTitle>
				<SheetDescription>
					{fmtTime(event.eventTime)}
					{#if event.region}· {event.region}{/if}
					{#if event.username}· {event.username}{/if}
				</SheetDescription>
			</SheetHeader>

			<div class="space-y-4 px-4 pb-6">
				<dl class="grid grid-cols-2 gap-x-4 gap-y-3 text-xs">
					<div>
						<dt class="text-muted-foreground">Event ID</dt>
						<dd class="mt-1 truncate font-mono">{event.eventId}</dd>
					</div>
					<div>
						<dt class="text-muted-foreground">Source</dt>
						<dd class="mt-1 font-mono">{event.eventSource}</dd>
					</div>
					{#if event.username}
						<div class="col-span-2">
							<dt class="text-muted-foreground">Username</dt>
							<dd class="mt-1 font-mono">{event.username}</dd>
						</div>
					{/if}
				</dl>

				{#if event.resources.length > 0}
					<Separator />
					<section>
						<h3 class="mb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
							Resources
						</h3>
						<ul class="space-y-1 text-xs">
							{#each event.resources as r, i (i)}
								<li class="flex items-center gap-2">
									{#if r.type}
										<Badge variant="outline" class="font-mono text-[10px]">{r.type}</Badge>
									{/if}
									<span class="font-mono text-foreground/80">{r.name ?? '—'}</span>
								</li>
							{/each}
						</ul>
					</section>
				{/if}

				{#if pretty}
					<Separator />
					<section>
						<header class="mb-2 flex items-center justify-between">
							<h3 class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
								Raw event
							</h3>
							<Button
								variant="ghost"
								size="sm"
								onclick={() => copy(pretty, 'Event JSON')}
								class="h-6 gap-1 px-2"
							>
								<Copy class="size-3" /> Copy
							</Button>
						</header>
						<pre
							class="overflow-x-auto rounded-md border border-border bg-muted/40 p-3 text-[11px] font-mono">{pretty}</pre>
					</section>
				{/if}
			</div>
		{/if}
	</SheetContent>
</Sheet>
