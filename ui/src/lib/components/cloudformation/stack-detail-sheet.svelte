<script lang="ts">
	import type { Stack } from '$lib/api/cloudformation';
	import { stackStatusVariant } from '$lib/api/cloudformation';
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';

	interface Props {
		stack: Stack | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { stack, open, onOpenChange }: Props = $props();

	function fmt(iso?: string): string {
		if (!iso) return '—';
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full overflow-y-auto sm:max-w-xl">
		{#if stack}
			<SheetHeader>
				<SheetTitle class="font-mono text-base">{stack.stackName}</SheetTitle>
				<SheetDescription>
					<Badge variant={stackStatusVariant(stack.stackStatus)}>{stack.stackStatus}</Badge>
				</SheetDescription>
			</SheetHeader>

			<div class="flex flex-col gap-4 p-4">
				<div class="grid grid-cols-2 gap-3 rounded-md border border-border bg-card p-3 text-sm">
					<div>
						<div class="text-xs text-muted-foreground">Created</div>
						<div class="mt-0.5 font-mono text-xs">{fmt(stack.creationTime)}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Last updated</div>
						<div class="mt-0.5 font-mono text-xs">{fmt(stack.lastUpdatedTime)}</div>
					</div>
					{#if stack.description}
						<div class="col-span-2">
							<div class="text-xs text-muted-foreground">Description</div>
							<div class="mt-0.5 text-xs">{stack.description}</div>
						</div>
					{/if}
					{#if stack.stackStatusReason}
						<div class="col-span-2">
							<div class="text-xs text-muted-foreground">Status reason</div>
							<div class="mt-0.5 text-xs">{stack.stackStatusReason}</div>
						</div>
					{/if}
					<div class="col-span-2">
						<div class="text-xs text-muted-foreground">Stack ID</div>
						<div class="mt-0.5 break-all font-mono text-[11px] text-muted-foreground">
							{stack.stackId}
						</div>
					</div>
				</div>

				{#if stack.capabilities.length > 0}
					<section class="rounded-md border border-border bg-card p-3">
						<h3 class="mb-2 text-sm font-medium">Capabilities</h3>
						<div class="flex flex-wrap gap-1.5">
							{#each stack.capabilities as c (c)}
								<Badge variant="outline" class="font-mono text-[10px]">{c}</Badge>
							{/each}
						</div>
					</section>
				{/if}

				{#if stack.tags.length > 0}
					<section class="rounded-md border border-border bg-card">
						<header class="border-b border-border px-3 py-2">
							<h3 class="text-sm font-medium">Tags ({stack.tags.length})</h3>
						</header>
						<ul class="divide-y divide-border/40">
							{#each stack.tags as t (t.key)}
								<li class="flex items-center justify-between gap-2 px-3 py-2">
									<span class="font-mono text-xs">{t.key}</span>
									<span class="font-mono text-[11px] text-muted-foreground">{t.value}</span>
								</li>
							{/each}
						</ul>
					</section>
				{/if}
			</div>
		{/if}
	</SheetContent>
</Sheet>
