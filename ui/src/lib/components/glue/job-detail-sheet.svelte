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
	import { getJob, type GlueJob } from '$lib/api/glue';
	import { toast } from 'svelte-sonner';

	interface Props {
		open: boolean;
		name: string | null;
		onOpenChange: (open: boolean) => void;
	}

	let { open, name, onOpenChange }: Props = $props();

	let detail = $state<GlueJob | null>(null);
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
			detail = await getJob(n);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load job');
		} finally {
			loading = false;
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-lg">
		<SheetHeader>
			<SheetTitle>Job</SheetTitle>
			<SheetDescription>
				{#if name}<span class="font-mono text-xs">{name}</span>{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4">
			{#if loading}
				<p class="text-xs text-muted-foreground">Loading…</p>
			{:else if detail}
				<dl class="grid grid-cols-[140px_1fr] gap-x-3 gap-y-1.5 text-xs">
					<dt class="text-muted-foreground">Command</dt>
					<dd>
						{#if detail.command.name}
							<Badge variant="outline" class="h-4 px-1 text-[10px]">{detail.command.name}</Badge>
						{:else}—{/if}
					</dd>
					<dt class="text-muted-foreground">Script</dt>
					<dd class="font-mono break-all text-[11px]">
						{detail.command.scriptLocation || '—'}
					</dd>
					<dt class="text-muted-foreground">Python version</dt>
					<dd>{detail.command.pythonVersion || '—'}</dd>
					<dt class="text-muted-foreground">Glue version</dt>
					<dd>{detail.glueVersion ?? '—'}</dd>
					<dt class="text-muted-foreground">Worker type</dt>
					<dd>
						{detail.workerType ?? '—'}
						{#if detail.numberOfWorkers}
							<span class="text-muted-foreground"> × {detail.numberOfWorkers}</span>
						{/if}
					</dd>
					<dt class="text-muted-foreground">Timeout (min)</dt>
					<dd>{detail.timeout ?? '—'}</dd>
					<dt class="text-muted-foreground">Max retries</dt>
					<dd>{detail.maxRetries ?? '—'}</dd>
					<dt class="text-muted-foreground">Role</dt>
					<dd class="font-mono break-all text-[11px]">{detail.role}</dd>
					<dt class="text-muted-foreground">Created</dt>
					<dd>{detail.createdOn ?? '—'}</dd>
					<dt class="text-muted-foreground">Modified</dt>
					<dd>{detail.lastModifiedOn ?? '—'}</dd>
				</dl>
			{/if}
		</div>

		<div class="flex justify-end gap-2 border-t border-border px-4 py-3">
			<Button variant="outline" onclick={() => onOpenChange(false)}>Close</Button>
		</div>
	</SheetContent>
</Sheet>
