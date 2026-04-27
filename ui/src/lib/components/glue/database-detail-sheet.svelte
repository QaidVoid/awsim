<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription,
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { getDatabase, type GlueDatabase } from '$lib/api/glue';
	import { toast } from 'svelte-sonner';

	interface Props {
		open: boolean;
		name: string | null;
		onOpenChange: (open: boolean) => void;
	}

	let { open, name, onOpenChange }: Props = $props();

	let detail = $state<GlueDatabase | null>(null);
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
			detail = await getDatabase(n);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load database');
		} finally {
			loading = false;
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-lg">
		<SheetHeader>
			<SheetTitle>Database</SheetTitle>
			<SheetDescription>
				{#if name}<span class="font-mono text-xs">{name}</span>{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4">
			{#if loading}
				<p class="text-xs text-muted-foreground">Loading…</p>
			{:else if detail}
				<dl class="grid grid-cols-[140px_1fr] gap-x-3 gap-y-1.5 text-xs">
					<dt class="text-muted-foreground">Name</dt>
					<dd class="font-medium">{detail.name}</dd>
					<dt class="text-muted-foreground">Description</dt>
					<dd>{detail.description || '—'}</dd>
					<dt class="text-muted-foreground">Location</dt>
					<dd class="font-mono break-all text-[11px]">{detail.locationUri || '—'}</dd>
					<dt class="text-muted-foreground">Created</dt>
					<dd>{detail.createTime ?? '—'}</dd>
				</dl>

				{#if Object.keys(detail.parameters).length > 0}
					<section>
						<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">
							Parameters
						</h3>
						<dl class="grid grid-cols-[140px_1fr] gap-x-3 gap-y-1 text-xs">
							{#each Object.entries(detail.parameters) as [k, v] (k)}
								<dt class="font-mono text-muted-foreground">{k}</dt>
								<dd class="font-mono break-all">{v}</dd>
							{/each}
						</dl>
					</section>
				{/if}
			{/if}
		</div>

		<div class="flex justify-end gap-2 border-t border-border px-4 py-3">
			<Button variant="outline" onclick={() => onOpenChange(false)}>Close</Button>
		</div>
	</SheetContent>
</Sheet>
