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
	import { getGuardrail, type GuardrailDetail } from '$lib/api/bedrock';
	import { toast } from 'svelte-sonner';

	interface Props {
		open: boolean;
		guardrailId: string | null;
		onOpenChange: (open: boolean) => void;
	}

	let { open, guardrailId, onOpenChange }: Props = $props();

	let detail = $state<GuardrailDetail | null>(null);
	let loading = $state(false);
	let lastFetched = $state<string | null>(null);

	$effect(() => {
		if (open && guardrailId && guardrailId !== lastFetched) {
			load(guardrailId);
		}
		if (!open) {
			detail = null;
			lastFetched = null;
		}
	});

	async function load(id: string) {
		loading = true;
		lastFetched = id;
		try {
			detail = await getGuardrail(id);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load guardrail');
			detail = null;
		} finally {
			loading = false;
		}
	}

	function statusVariant(s: string): 'secondary' | 'destructive' | 'outline' {
		if (s === 'READY') return 'secondary';
		if (s === 'FAILED') return 'destructive';
		return 'outline';
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-lg">
		<SheetHeader>
			<SheetTitle>Guardrail details</SheetTitle>
			<SheetDescription>
				{#if guardrailId}
					<span class="font-mono text-xs">{guardrailId}</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4">
			{#if loading}
				<p class="px-2 py-2 text-xs text-muted-foreground">Loading…</p>
			{:else if detail}
				<dl class="grid grid-cols-[140px_1fr] gap-x-3 gap-y-1.5 text-xs">
					<dt class="text-muted-foreground">Name</dt>
					<dd class="font-medium">{detail.name}</dd>
					<dt class="text-muted-foreground">Status</dt>
					<dd>
						<Badge variant={statusVariant(detail.status)} class="text-[10px]">
							{detail.status || '—'}
						</Badge>
					</dd>
					<dt class="text-muted-foreground">Version</dt>
					<dd class="font-mono">{detail.version}</dd>
					<dt class="text-muted-foreground">Description</dt>
					<dd>{detail.description ?? '—'}</dd>
					<dt class="text-muted-foreground">ARN</dt>
					<dd class="font-mono break-all text-[10px]">{detail.arn}</dd>
					<dt class="text-muted-foreground">Created</dt>
					<dd>{detail.createdAt ?? '—'}</dd>
					<dt class="text-muted-foreground">Updated</dt>
					<dd>{detail.updatedAt ?? '—'}</dd>
				</dl>

				<section>
					<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">
						Blocked input message
					</h3>
					<pre
						class="overflow-auto rounded-md border border-border bg-muted/40 p-2 text-xs whitespace-pre-wrap break-words">{detail.blockedInputMessaging || '—'}</pre>
				</section>

				<section>
					<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">
						Blocked output message
					</h3>
					<pre
						class="overflow-auto rounded-md border border-border bg-muted/40 p-2 text-xs whitespace-pre-wrap break-words">{detail.blockedOutputsMessaging || '—'}</pre>
				</section>
			{/if}
		</div>

		<div class="flex justify-end gap-2 border-t border-border px-4 py-3">
			<Button variant="outline" onclick={() => onOpenChange(false)}>Close</Button>
		</div>
	</SheetContent>
</Sheet>
