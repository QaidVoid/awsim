<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import GaugeIcon from '@lucide/svelte/icons/gauge';
	import { toast } from 'svelte-sonner';
	import {
		describeScalableTargets,
		deregisterScalableTarget,
		describeScalingPolicies,
		SERVICE_NAMESPACES,
		type ScalableTarget,
		type ScalingPolicy
	} from '$lib/api/application-autoscaling';

	interface Props {
		serviceNamespace: string;
		onCreate: () => void;
		onSelect: (t: ScalableTarget) => void;
		refreshKey?: number;
	}

	let { serviceNamespace = $bindable(), onCreate, onSelect, refreshKey = 0 }: Props = $props();

	let targets = $state<ScalableTarget[]>([]);
	let policiesByTarget = $state<Record<string, ScalingPolicy[]>>({});
	let loading = $state(false);

	$effect(() => {
		serviceNamespace;
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			targets = await describeScalableTargets(serviceNamespace);
			const policies = await describeScalingPolicies(serviceNamespace);
			const byTarget: Record<string, ScalingPolicy[]> = {};
			for (const p of policies) {
				const k = `${p.resourceId}|${p.scalableDimension}`;
				byTarget[k] = byTarget[k] ?? [];
				byTarget[k].push(p);
			}
			policiesByTarget = byTarget;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load scalable targets');
		} finally {
			loading = false;
		}
	}

	async function deregister(t: ScalableTarget) {
		if (
			!confirm(
				`Deregister scalable target ${t.resourceId} / ${t.scalableDimension}? Attached policies will also be removed.`
			)
		)
			return;
		try {
			await deregisterScalableTarget({
				serviceNamespace: t.serviceNamespace,
				resourceId: t.resourceId,
				scalableDimension: t.scalableDimension
			});
			toast.success('Deregistered scalable target.');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to deregister');
		}
	}

	function policyCount(t: ScalableTarget): number {
		return policiesByTarget[`${t.resourceId}|${t.scalableDimension}`]?.length ?? 0;
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="flex items-center gap-2">
			<label for="aas-ns" class="text-xs text-muted-foreground">Namespace</label>
			<select
				id="aas-ns"
				bind:value={serviceNamespace}
				class="h-7 rounded-md border border-border bg-background px-2 text-xs"
			>
				{#each SERVICE_NAMESPACES as ns (ns)}
					<option value={ns}>{ns}</option>
				{/each}
			</select>
		</div>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={onCreate}>
				<PlusIcon />
				Register target
			</Button>
		</div>
	</div>

	<DataTable
		rows={targets}
		{loading}
		onRowClick={onSelect}
		columns={[
			{ key: 'resourceId', label: 'Resource', mono: true },
			{ key: 'scalableDimension', label: 'Dimension', mono: true },
			{ key: 'minCapacity', label: 'Min', width: '60px' },
			{ key: 'maxCapacity', label: 'Max', width: '60px' },
			{ key: 'resourceId', label: 'Policies', width: '90px', cell: policyCell },
			{ key: 'resourceId', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => `${r.resourceId}|${r.scalableDimension}`}
	>
		{#snippet empty()}
			<EmptyState
				icon={GaugeIcon}
				title="No scalable targets"
				description="Register an ECS service / Lambda alias / DynamoDB table as a scalable target to attach scaling policies."
			>
				{#snippet action()}
					<Button onclick={onCreate}>
						<PlusIcon />
						Register target
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

{#snippet policyCell(row: ScalableTarget)}
	<Badge variant="outline" class="h-5 px-2 text-[10px]">{policyCount(row)}</Badge>
{/snippet}

{#snippet actionsCell(row: ScalableTarget)}
	<Button variant="ghost" size="xs" onclick={() => deregister(row)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}
