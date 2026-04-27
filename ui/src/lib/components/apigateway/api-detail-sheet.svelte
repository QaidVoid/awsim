<script lang="ts">
	import type { RestApi } from '$lib/api/apigateway';
	import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetDescription } from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';

	interface Props {
		open: boolean;
		api: RestApi | null;
		onOpenChange: (o: boolean) => void;
	}

	let { open, api, onOpenChange }: Props = $props();

	function formatDate(iso: string): string {
		if (!iso) return '—';
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent class="w-[420px] sm:max-w-md">
		<SheetHeader>
			<SheetTitle>{api?.name || 'REST API'}</SheetTitle>
			<SheetDescription>Full API metadata</SheetDescription>
		</SheetHeader>
		{#if api}
			<div class="px-6 pb-6">
				<dl class="grid grid-cols-[100px_1fr] gap-x-3 gap-y-2 text-xs">
					<dt class="text-muted-foreground">ID</dt>
					<dd class="font-mono">{api.id}</dd>

					<dt class="text-muted-foreground">Name</dt>
					<dd>{api.name || '—'}</dd>

					<dt class="text-muted-foreground">Description</dt>
					<dd>{api.description || '—'}</dd>

					<dt class="text-muted-foreground">Version</dt>
					<dd>{api.version || '—'}</dd>

					<dt class="text-muted-foreground">Created</dt>
					<dd>{formatDate(api.createdDate)}</dd>

					<dt class="text-muted-foreground">API key source</dt>
					<dd>{api.apiKeySource || '—'}</dd>

					<dt class="text-muted-foreground">Endpoint types</dt>
					<dd>
						{#if api.endpointTypes.length}
							<div class="flex flex-wrap gap-1">
								{#each api.endpointTypes as t (t)}
									<Badge variant="outline" class="h-4 px-1 text-[10px]">{t}</Badge>
								{/each}
							</div>
						{:else}
							—
						{/if}
					</dd>
				</dl>
			</div>
		{/if}
	</SheetContent>
</Sheet>
