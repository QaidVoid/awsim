<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle,
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import { toast } from 'svelte-sonner';
	import {
		getDistribution,
		type Distribution,
		type DistributionDetail,
	} from '$lib/api/cloudfront';

	interface Props {
		distribution: Distribution | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { distribution, open = $bindable(), onOpenChange }: Props = $props();

	let detail = $state<DistributionDetail | null>(null);
	let loading = $state(false);

	$effect(() => {
		if (distribution && open) void load(distribution.id);
	});

	async function load(id: string) {
		loading = true;
		detail = null;
		try {
			detail = await getDistribution(id);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load distribution');
		} finally {
			loading = false;
		}
	}

	async function copy(value: string, label: string) {
		try {
			await navigator.clipboard.writeText(value);
			toast.success(`${label} copied.`);
		} catch {
			toast.error('Copy failed.');
		}
	}
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent side="right" class="w-full max-w-xl overflow-y-auto sm:max-w-xl">
		<SheetHeader>
			<SheetTitle>{distribution?.id ?? ''}</SheetTitle>
			<SheetDescription class="font-mono text-xs">{distribution?.arn ?? ''}</SheetDescription>
		</SheetHeader>

		{#if distribution}
			<div class="flex flex-col gap-4 px-6 pb-6">
				<section class="rounded-md border border-border bg-card/40 p-4">
					<h3 class="mb-3 text-sm font-semibold">Configuration</h3>
					<dl class="grid grid-cols-[160px_1fr] gap-x-4 gap-y-2 text-xs">
						<dt class="text-muted-foreground">Status</dt>
						<dd>
							<Badge variant="outline" class="h-4 px-1.5 text-[10px]">
								{distribution.status}
							</Badge>
						</dd>
						<dt class="text-muted-foreground">Enabled</dt>
						<dd>
							{#if distribution.enabled}
								<Badge variant="outline" class="h-4 px-1.5 text-[10px] text-green-500"
									>enabled</Badge
								>
							{:else}
								<Badge variant="outline" class="h-4 px-1.5 text-[10px]">disabled</Badge>
							{/if}
						</dd>
						<dt class="text-muted-foreground">Domain</dt>
						<dd class="font-mono text-[11px] break-all">{distribution.domainName || '—'}</dd>
						<dt class="text-muted-foreground">Origin</dt>
						<dd class="font-mono text-[11px] break-all">
							{distribution.originDomainName || '—'}
						</dd>
						<dt class="text-muted-foreground">Price class</dt>
						<dd>{distribution.priceClass || '—'}</dd>
						<dt class="text-muted-foreground">HTTP version</dt>
						<dd>{distribution.httpVersion || '—'}</dd>
						{#if detail?.defaultRootObject}
							<dt class="text-muted-foreground">Default root</dt>
							<dd class="font-mono text-[11px]">{detail.defaultRootObject}</dd>
						{/if}
						<dt class="text-muted-foreground">Comment</dt>
						<dd>{distribution.comment || '—'}</dd>
						<dt class="text-muted-foreground">Last modified</dt>
						<dd>{distribution.lastModifiedTime || '—'}</dd>
					</dl>
					{#if loading}
						<p class="mt-3 text-[11px] text-muted-foreground">Loading details…</p>
					{/if}
				</section>

				<section class="rounded-md border border-border bg-card/40 p-4">
					<div class="mb-2 flex items-center justify-between">
						<h3 class="text-sm font-semibold">Domain</h3>
						<Button
							variant="ghost"
							size="xs"
							onclick={() => copy(distribution.domainName, 'Domain')}
						>
							<CopyIcon />
							Copy
						</Button>
					</div>
					<pre
						class="overflow-auto rounded-md border border-border bg-muted/40 p-3 text-[11px] font-mono break-all whitespace-pre-wrap">https://{distribution.domainName}/</pre>
				</section>
			</div>
		{/if}
	</SheetContent>
</Sheet>
