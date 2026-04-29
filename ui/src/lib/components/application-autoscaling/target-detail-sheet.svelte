<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { toast } from 'svelte-sonner';
	import {
		describeScalingPolicies,
		deleteScalingPolicy,
		type ScalableTarget,
		type ScalingPolicy
	} from '$lib/api/application-autoscaling';

	interface Props {
		open: boolean;
		target: ScalableTarget | null;
		onOpenChange: (open: boolean) => void;
		onChanged?: () => void;
	}

	let { open, target, onOpenChange, onChanged }: Props = $props();

	let policies = $state<ScalingPolicy[]>([]);
	let loading = $state(false);

	$effect(() => {
		if (open && target) {
			void load(target);
		} else if (!open) {
			policies = [];
		}
	});

	async function load(t: ScalableTarget) {
		loading = true;
		try {
			const all = await describeScalingPolicies(t.serviceNamespace, t.resourceId);
			policies = all.filter((p) => p.scalableDimension === t.scalableDimension);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load policies');
		} finally {
			loading = false;
		}
	}

	async function removePolicy(p: ScalingPolicy) {
		if (!confirm(`Delete policy "${p.policyName}"?`)) return;
		try {
			await deleteScalingPolicy({
				serviceNamespace: p.serviceNamespace,
				resourceId: p.resourceId,
				scalableDimension: p.scalableDimension,
				policyName: p.policyName
			});
			toast.success('Policy deleted.');
			if (target) await load(target);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete policy');
		}
	}

	function timestamp(t: number): string {
		return new Date(t * 1000).toLocaleString();
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-xl">
		<SheetHeader>
			<SheetTitle>Scalable target</SheetTitle>
			<SheetDescription>
				{#if target}
					<span class="font-mono text-xs">
						{target.serviceNamespace} • {target.resourceId} • {target.scalableDimension}
					</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 pb-4">
			{#if target}
				<div class="grid grid-cols-2 gap-3 text-xs">
					<div>
						<div class="font-semibold text-muted-foreground">Min capacity</div>
						<div>{target.minCapacity}</div>
					</div>
					<div>
						<div class="font-semibold text-muted-foreground">Max capacity</div>
						<div>{target.maxCapacity}</div>
					</div>
					<div>
						<div class="font-semibold text-muted-foreground">Created</div>
						<div>{timestamp(target.creationTime)}</div>
					</div>
					<div>
						<div class="font-semibold text-muted-foreground">Role ARN</div>
						<div class="font-mono text-[11px] break-all">{target.roleArn}</div>
					</div>
				</div>

				<div class="space-y-2">
					<div class="text-xs font-semibold text-muted-foreground">
						Scaling policies ({policies.length})
					</div>
					{#if loading}
						<p class="text-xs text-muted-foreground">Loading…</p>
					{:else if policies.length === 0}
						<div class="rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground">
							No scaling policies. Attach via the AWS CLI or SDK.
						</div>
					{:else}
						<div class="space-y-1.5">
							{#each policies as p (p.policyArn)}
								<div class="flex items-center justify-between rounded-md border border-border p-2 text-xs">
									<div class="flex flex-col gap-0.5">
										<span class="font-mono">{p.policyName}</span>
										<Badge variant="outline" class="h-4 w-fit px-2 text-[10px]">
											{p.policyType}
										</Badge>
									</div>
									<Button variant="ghost" size="xs" onclick={() => removePolicy(p)}>
										<Trash2Icon class="text-destructive" />
									</Button>
								</div>
							{/each}
						</div>
					{/if}
				</div>
			{/if}
		</div>
	</SheetContent>
</Sheet>
