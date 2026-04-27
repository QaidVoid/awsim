<script lang="ts">
	import { Badge } from '$lib/components/ui/badge';
	import type { TopicAttributes } from '$lib/api/sns';

	interface Props {
		attrs: TopicAttributes;
	}

	let { attrs }: Props = $props();

	function prettyJson(s: string): string {
		try {
			return JSON.stringify(JSON.parse(s), null, 2);
		} catch {
			return s || '—';
		}
	}
</script>

<div class="flex flex-col gap-4 p-4">
	<section class="rounded-md border border-border bg-card/40 p-4">
		<h3 class="mb-3 text-sm font-semibold">Identity</h3>
		<dl class="grid grid-cols-[160px_1fr] gap-x-4 gap-y-2 text-xs">
			<dt class="text-muted-foreground">Type</dt>
			<dd>
				{#if attrs.isFifo}
					<Badge variant="outline" class="h-4 px-1.5 text-[10px]">FIFO</Badge>
				{:else}
					<Badge variant="outline" class="h-4 px-1.5 text-[10px]">Standard</Badge>
				{/if}
			</dd>
			<dt class="text-muted-foreground">ARN</dt>
			<dd class="font-mono text-[11px] break-all">{attrs.arn || '—'}</dd>
			<dt class="text-muted-foreground">Display name</dt>
			<dd>{attrs.displayName || '—'}</dd>
			<dt class="text-muted-foreground">Subscriptions</dt>
			<dd>
				<span class="font-medium">{attrs.subscriptionsConfirmed}</span> confirmed ·
				<span class="text-amber-500">{attrs.subscriptionsPending}</span> pending ·
				<span class="text-muted-foreground">{attrs.subscriptionsDeleted}</span> deleted
			</dd>
			{#if attrs.isFifo}
				<dt class="text-muted-foreground">Content-based dedup</dt>
				<dd>{attrs.contentBasedDeduplication ? 'Enabled' : 'Disabled'}</dd>
			{/if}
		</dl>
	</section>

	<section class="rounded-md border border-border bg-card/40 p-4">
		<h3 class="mb-3 text-sm font-semibold">Access policy</h3>
		<pre
			class="max-h-96 overflow-auto rounded-md border border-border bg-muted/40 p-3 text-[11px] font-mono whitespace-pre-wrap break-all">{prettyJson(
				attrs.policy
			)}</pre>
	</section>
</div>
