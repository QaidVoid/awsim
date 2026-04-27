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
	import SendIcon from '@lucide/svelte/icons/send';
	import type { DeliveryStreamDetail } from '$lib/api/firehose';

	interface Props {
		open: boolean;
		stream: DeliveryStreamDetail | null;
		onOpenChange: (open: boolean) => void;
		onPutRecord: () => void;
	}

	let { open, stream, onOpenChange, onPutRecord }: Props = $props();
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle>
				{#if stream}<span class="font-mono">{stream.name}</span>{:else}Stream{/if}
			</SheetTitle>
			<SheetDescription>
				{#if stream}<span class="font-mono text-xs">{stream.arn}</span>{/if}
			</SheetDescription>
		</SheetHeader>

		{#if stream}
			<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4">
				<section class="rounded-md border border-border bg-card/40 p-3">
					<dl class="grid grid-cols-2 gap-3 text-xs sm:grid-cols-3">
						<div>
							<dt class="text-muted-foreground">Status</dt>
							<dd>
								<Badge variant="outline" class="h-4 px-1.5 text-[10px]">{stream.status}</Badge>
							</dd>
						</div>
						<div>
							<dt class="text-muted-foreground">Type</dt>
							<dd>{stream.type}</dd>
						</div>
						<div>
							<dt class="text-muted-foreground">Version</dt>
							<dd class="font-mono">{stream.versionId}</dd>
						</div>
						<div>
							<dt class="text-muted-foreground">Created</dt>
							<dd>
								{stream.createTime
									? new Date(stream.createTime * 1000).toLocaleString()
									: '—'}
							</dd>
						</div>
						<div>
							<dt class="text-muted-foreground">Updated</dt>
							<dd>
								{stream.lastUpdate
									? new Date(stream.lastUpdate * 1000).toLocaleString()
									: '—'}
							</dd>
						</div>
					</dl>
					<div class="mt-3 flex justify-end">
						<Button size="sm" onclick={onPutRecord}>
							<SendIcon />
							Put record
						</Button>
					</div>
				</section>

				<section>
					<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">
						Destinations
					</h3>
					{#if stream.destinations.length === 0}
						<p class="text-xs text-muted-foreground">No destinations configured.</p>
					{:else}
						<ul class="flex flex-col gap-2">
							{#each stream.destinations as dest (dest.destinationId)}
								<li class="rounded-md border border-border bg-card/40 p-3">
									<div class="flex items-center justify-between">
										<Badge variant="outline" class="h-5 px-2 text-[10px]">
											{dest.type}
										</Badge>
										<span class="font-mono text-[10px] text-muted-foreground">
											{dest.destinationId}
										</span>
									</div>
									<dl class="mt-2 grid grid-cols-[140px_1fr] gap-x-3 gap-y-1 text-xs">
										{#if dest.bucketArn}
											<dt class="text-muted-foreground">Bucket ARN</dt>
											<dd class="font-mono break-all">{dest.bucketArn}</dd>
										{/if}
										{#if dest.prefix}
											<dt class="text-muted-foreground">Prefix</dt>
											<dd class="font-mono">{dest.prefix}</dd>
										{/if}
										{#if dest.errorOutputPrefix}
											<dt class="text-muted-foreground">Error prefix</dt>
											<dd class="font-mono">{dest.errorOutputPrefix}</dd>
										{/if}
										{#if dest.compressionFormat}
											<dt class="text-muted-foreground">Compression</dt>
											<dd>{dest.compressionFormat}</dd>
										{/if}
										{#if dest.bufferingHints}
											<dt class="text-muted-foreground">Buffering</dt>
											<dd>
												{dest.bufferingHints.sizeInMBs} MB or {dest.bufferingHints.intervalInSeconds}s
											</dd>
										{/if}
										{#if dest.endpointUrl}
											<dt class="text-muted-foreground">Endpoint URL</dt>
											<dd class="font-mono break-all">{dest.endpointUrl}</dd>
										{/if}
									</dl>
								</li>
							{/each}
						</ul>
					{/if}
				</section>
			</div>
		{/if}
	</SheetContent>
</Sheet>
