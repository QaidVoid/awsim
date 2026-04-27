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
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { toast } from 'svelte-sonner';
	import {
		getHostedZone,
		type HostedZone,
		type HostedZoneDetail,
	} from '$lib/api/route53';

	interface Props {
		zone: HostedZone | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onDelete?: (id: string) => void;
	}

	let { zone, open = $bindable(), onOpenChange, onDelete }: Props = $props();

	let detail = $state<HostedZoneDetail | null>(null);
	let loading = $state(false);

	$effect(() => {
		if (zone && open) void load(zone.id);
	});

	async function load(id: string) {
		loading = true;
		detail = null;
		try {
			detail = await getHostedZone(id);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load zone');
		} finally {
			loading = false;
		}
	}
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent side="right" class="w-full max-w-xl overflow-y-auto sm:max-w-xl">
		<SheetHeader>
			<SheetTitle>{zone?.name ?? ''}</SheetTitle>
			<SheetDescription class="font-mono text-xs">{zone?.id ?? ''}</SheetDescription>
		</SheetHeader>

		{#if zone}
			<div class="flex flex-col gap-4 px-6 pb-6">
				<section class="rounded-md border border-border bg-card/40 p-4">
					<h3 class="mb-3 text-sm font-semibold">Identity</h3>
					<dl class="grid grid-cols-[140px_1fr] gap-x-4 gap-y-2 text-xs">
						<dt class="text-muted-foreground">Type</dt>
						<dd>
							{#if zone.privateZone}
								<Badge variant="outline" class="h-4 px-1.5 text-[10px]">PRIVATE</Badge>
							{:else}
								<Badge variant="outline" class="h-4 px-1.5 text-[10px]">PUBLIC</Badge>
							{/if}
						</dd>
						<dt class="text-muted-foreground">Records</dt>
						<dd>{zone.resourceRecordSetCount}</dd>
						{#if zone.comment}
							<dt class="text-muted-foreground">Comment</dt>
							<dd>{zone.comment}</dd>
						{/if}
						<dt class="text-muted-foreground">Caller ref</dt>
						<dd class="font-mono text-[11px] break-all">{zone.callerReference}</dd>
					</dl>
				</section>

				<section class="rounded-md border border-border bg-card/40 p-4">
					<h3 class="mb-3 text-sm font-semibold">Name servers</h3>
					{#if loading}
						<p class="text-xs text-muted-foreground">Loading…</p>
					{:else if detail && detail.nameServers.length > 0}
						<ul class="flex flex-col gap-1">
							{#each detail.nameServers as ns (ns)}
								<li class="font-mono text-[11px]">{ns}</li>
							{/each}
						</ul>
					{:else}
						<p class="text-xs text-muted-foreground">No name servers reported.</p>
					{/if}
				</section>

				<div class="flex justify-end">
					<Button variant="destructive" size="sm" onclick={() => onDelete?.(zone.id)}>
						<Trash2Icon />
						Delete zone
					</Button>
				</div>
			</div>
		{/if}
	</SheetContent>
</Sheet>
