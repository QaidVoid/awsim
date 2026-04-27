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
	import { getConnection, type GlueConnection } from '$lib/api/glue';
	import { toast } from 'svelte-sonner';

	interface Props {
		open: boolean;
		name: string | null;
		onOpenChange: (open: boolean) => void;
	}

	let { open, name, onOpenChange }: Props = $props();

	let detail = $state<GlueConnection | null>(null);
	let loading = $state(false);
	let lastFetched = $state<string | null>(null);

	const SECRET_KEYS = ['PASSWORD', 'SECRET_ACCESS_KEY', 'SECRET'];

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
			detail = await getConnection(n);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load connection');
		} finally {
			loading = false;
		}
	}

	function isSecret(k: string): boolean {
		const upper = k.toUpperCase();
		return SECRET_KEYS.some((s) => upper.includes(s));
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-lg">
		<SheetHeader>
			<SheetTitle>Connection</SheetTitle>
			<SheetDescription>
				{#if name}<span class="font-mono text-xs">{name}</span>{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4">
			{#if loading}
				<p class="text-xs text-muted-foreground">Loading…</p>
			{:else if detail}
				<dl class="grid grid-cols-[140px_1fr] gap-x-3 gap-y-1.5 text-xs">
					<dt class="text-muted-foreground">Type</dt>
					<dd>
						{#if detail.connectionType}
							<Badge variant="outline" class="h-4 px-1 text-[10px]">
								{detail.connectionType}
							</Badge>
						{:else}—{/if}
					</dd>
					<dt class="text-muted-foreground">Description</dt>
					<dd>{detail.description || '—'}</dd>
					<dt class="text-muted-foreground">Created</dt>
					<dd>{detail.creationTime ?? '—'}</dd>
					<dt class="text-muted-foreground">Updated</dt>
					<dd>{detail.lastUpdatedTime ?? '—'}</dd>
				</dl>

				{#if detail.matchCriteria.length > 0}
					<section>
						<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">
							Match criteria
						</h3>
						<div class="flex flex-wrap gap-1">
							{#each detail.matchCriteria as c (c)}
								<Badge variant="outline" class="h-4 px-1 text-[10px]">{c}</Badge>
							{/each}
						</div>
					</section>
				{/if}

				{#if Object.keys(detail.properties).length > 0}
					<section>
						<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">
							Properties
						</h3>
						<dl class="grid grid-cols-[160px_1fr] gap-x-3 gap-y-1 text-xs">
							{#each Object.entries(detail.properties) as [k, v] (k)}
								<dt class="font-mono text-muted-foreground">{k}</dt>
								<dd class="font-mono break-all">
									{isSecret(k) ? '••••••••' : v}
								</dd>
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
