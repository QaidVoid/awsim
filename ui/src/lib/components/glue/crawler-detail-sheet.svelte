<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription,
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { getCrawler, type GlueCrawler } from '$lib/api/glue';
	import { toast } from 'svelte-sonner';

	interface Props {
		open: boolean;
		name: string | null;
		onOpenChange: (open: boolean) => void;
	}

	let { open, name, onOpenChange }: Props = $props();

	let detail = $state<GlueCrawler | null>(null);
	let loading = $state(false);
	let lastFetched = $state<string | null>(null);

	$effect(() => {
		if (open && name && name !== lastFetched) {
			load(name);
		}
		if (!open) {
			detail = null;
			lastFetched = null;
		}
	});

	async function load(n: string) {
		loading = true;
		lastFetched = n;
		try {
			detail = await getCrawler(n);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load crawler');
		} finally {
			loading = false;
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-lg">
		<SheetHeader>
			<SheetTitle>Crawler</SheetTitle>
			<SheetDescription>
				{#if name}<span class="font-mono text-xs">{name}</span>{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4">
			{#if loading}
				<p class="text-xs text-muted-foreground">Loading…</p>
			{:else if detail}
				<dl class="grid grid-cols-[140px_1fr] gap-x-3 gap-y-1.5 text-xs">
					<dt class="text-muted-foreground">State</dt>
					<dd>
						<Badge variant="outline" class="h-4 px-1 text-[10px]">{detail.state || '—'}</Badge>
					</dd>
					<dt class="text-muted-foreground">Database</dt>
					<dd>{detail.databaseName ?? '—'}</dd>
					<dt class="text-muted-foreground">Role</dt>
					<dd class="font-mono break-all text-[11px]">{detail.role}</dd>
					<dt class="text-muted-foreground">Schedule</dt>
					<dd class="font-mono text-[11px]">{detail.schedule ?? '—'}</dd>
					<dt class="text-muted-foreground">Last crawl</dt>
					<dd>
						{detail.lastCrawlState ?? '—'}
						{#if detail.lastCrawlTime}
							<span class="text-muted-foreground"> · {detail.lastCrawlTime}</span>
						{/if}
					</dd>
					<dt class="text-muted-foreground">Table prefix</dt>
					<dd>{detail.tablePrefix ?? '—'}</dd>
				</dl>

				<section>
					<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">Targets</h3>
					{#if detail.targets.length === 0}
						<p class="text-xs text-muted-foreground">No targets.</p>
					{:else}
						<ul class="flex flex-col gap-1 text-xs">
							{#each detail.targets as t, i (i)}
								<li class="flex items-start gap-2">
									<Badge variant="outline" class="h-4 px-1 text-[10px]">{t.type}</Badge>
									<span class="font-mono break-all">{t.path}</span>
								</li>
							{/each}
						</ul>
					{/if}
				</section>
			{/if}
		</div>

		<div class="flex justify-end gap-2 border-t border-border px-4 py-3">
			<Button variant="outline" onclick={() => onOpenChange(false)}>Close</Button>
		</div>
	</SheetContent>
</Sheet>
