<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription,
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import { toast } from 'svelte-sonner';
	import type { FoundationModel } from '$lib/api/bedrock';

	interface Props {
		open: boolean;
		model: FoundationModel | null;
		onOpenChange: (open: boolean) => void;
	}

	let { open, model, onOpenChange }: Props = $props();

	async function copy(text: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast.success('Copied.');
		} catch {
			toast.error('Copy failed.');
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-lg">
		<SheetHeader>
			<SheetTitle>Model details</SheetTitle>
			<SheetDescription>
				{#if model}
					<span class="font-mono text-xs">{model.modelId}</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		{#if model}
			<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4">
				<section>
					<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">Identity</h3>
					<dl class="grid grid-cols-[120px_1fr] gap-x-3 gap-y-1.5 text-xs">
						<dt class="text-muted-foreground">Name</dt>
						<dd class="font-medium">{model.modelName || '—'}</dd>
						<dt class="text-muted-foreground">Provider</dt>
						<dd>{model.providerName || '—'}</dd>
						<dt class="text-muted-foreground">Model ID</dt>
						<dd class="flex items-center gap-1">
							<span class="font-mono break-all">{model.modelId}</span>
							<Button variant="ghost" size="xs" onclick={() => copy(model.modelId)}>
								<CopyIcon class="size-3" />
							</Button>
						</dd>
						<dt class="text-muted-foreground">ARN</dt>
						<dd class="flex items-center gap-1">
							<span class="font-mono break-all text-[10px]">{model.modelArn}</span>
							<Button variant="ghost" size="xs" onclick={() => copy(model.modelArn)}>
								<CopyIcon class="size-3" />
							</Button>
						</dd>
					</dl>
				</section>

				<section>
					<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">Modalities</h3>
					<div class="flex flex-col gap-2 text-xs">
						<div class="flex items-center gap-2">
							<span class="w-16 text-muted-foreground">Input</span>
							<div class="flex flex-wrap gap-1">
								{#each model.inputModalities as m (m)}
									<Badge variant="secondary" class="h-4 px-1 text-[10px]">{m}</Badge>
								{:else}
									<span class="text-muted-foreground">—</span>
								{/each}
							</div>
						</div>
						<div class="flex items-center gap-2">
							<span class="w-16 text-muted-foreground">Output</span>
							<div class="flex flex-wrap gap-1">
								{#each model.outputModalities as m (m)}
									<Badge variant="outline" class="h-4 px-1 text-[10px]">{m}</Badge>
								{:else}
									<span class="text-muted-foreground">—</span>
								{/each}
							</div>
						</div>
					</div>
				</section>

				<section>
					<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">
						Capabilities
					</h3>
					<dl class="grid grid-cols-[140px_1fr] gap-x-3 gap-y-1.5 text-xs">
						<dt class="text-muted-foreground">Streaming</dt>
						<dd>
							{#if model.responseStreamingSupported}
								<Badge variant="secondary" class="h-4 px-1 text-[10px]">supported</Badge>
							{:else}
								<span class="text-muted-foreground">—</span>
							{/if}
						</dd>
						<dt class="text-muted-foreground">Customizations</dt>
						<dd class="flex flex-wrap gap-1">
							{#each model.customizationsSupported ?? [] as c (c)}
								<Badge variant="outline" class="h-4 px-1 text-[10px]">{c}</Badge>
							{:else}
								<span class="text-muted-foreground">—</span>
							{/each}
						</dd>
						<dt class="text-muted-foreground">Inference</dt>
						<dd class="flex flex-wrap gap-1">
							{#each model.inferenceTypesSupported ?? [] as t (t)}
								<Badge variant="outline" class="h-4 px-1 text-[10px]">{t}</Badge>
							{:else}
								<span class="text-muted-foreground">—</span>
							{/each}
						</dd>
					</dl>
				</section>
			</div>

			<div class="flex justify-end gap-2 border-t border-border px-4 py-3">
				<Button variant="outline" onclick={() => onOpenChange(false)}>Close</Button>
			</div>
		{/if}
	</SheetContent>
</Sheet>
