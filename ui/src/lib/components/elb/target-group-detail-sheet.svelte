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
	import type { TargetGroup } from '$lib/api/elb';

	interface Props {
		tg: TargetGroup | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onDelete?: (arn: string) => void;
	}

	let { tg, open = $bindable(), onOpenChange, onDelete }: Props = $props();
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent side="right" class="w-full max-w-xl overflow-y-auto sm:max-w-xl">
		<SheetHeader>
			<SheetTitle>{tg?.name ?? ''}</SheetTitle>
			<SheetDescription class="truncate font-mono text-xs">{tg?.arn ?? ''}</SheetDescription>
		</SheetHeader>

		{#if tg}
			<div class="flex flex-col gap-4 px-6 pb-6">
				<section class="rounded-md border border-border bg-card/40 p-4">
					<h3 class="mb-3 text-sm font-semibold">Configuration</h3>
					<dl class="grid grid-cols-[160px_1fr] gap-x-4 gap-y-2 text-xs">
						<dt class="text-muted-foreground">Protocol / port</dt>
						<dd>{tg.protocol}:{tg.port}</dd>
						<dt class="text-muted-foreground">Target type</dt>
						<dd>
							<Badge variant="outline" class="h-4 px-1.5 text-[10px] uppercase">
								{tg.targetType}
							</Badge>
						</dd>
						<dt class="text-muted-foreground">VPC</dt>
						<dd class="font-mono text-[11px]">{tg.vpcId || '—'}</dd>
					</dl>
				</section>

				<section class="rounded-md border border-border bg-card/40 p-4">
					<h3 class="mb-3 text-sm font-semibold">Health check</h3>
					<dl class="grid grid-cols-[160px_1fr] gap-x-4 gap-y-2 text-xs">
						<dt class="text-muted-foreground">Protocol</dt>
						<dd>{tg.healthCheckProtocol || '—'}</dd>
						<dt class="text-muted-foreground">Path</dt>
						<dd class="font-mono text-[11px]">{tg.healthCheckPath || '—'}</dd>
					</dl>
				</section>

				<div class="flex justify-end">
					<Button variant="destructive" size="sm" onclick={() => onDelete?.(tg.arn)}>
						<Trash2Icon />
						Delete target group
					</Button>
				</div>
			</div>
		{/if}
	</SheetContent>
</Sheet>
