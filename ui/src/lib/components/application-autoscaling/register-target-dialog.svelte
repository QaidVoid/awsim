<script lang="ts">
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toast } from 'svelte-sonner';
	import {
		registerScalableTarget,
		SERVICE_NAMESPACES
	} from '$lib/api/application-autoscaling';

	interface Props {
		open: boolean;
		defaultNamespace?: string;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let {
		open,
		defaultNamespace = 'ecs',
		onOpenChange,
		onCreated
	}: Props = $props();

	let serviceNamespace = $state('ecs');
	let resourceId = $state('');
	let scalableDimension = $state('ecs:service:DesiredCount');
	let minCapacity = $state('1');
	let maxCapacity = $state('10');
	let creating = $state(false);

	$effect(() => {
		if (open) {
			serviceNamespace = defaultNamespace;
		}
	});

	async function submit() {
		if (!resourceId.trim() || !scalableDimension.trim()) {
			return toast.error('ResourceId and ScalableDimension are required.');
		}
		const min = parseInt(minCapacity, 10);
		const max = parseInt(maxCapacity, 10);
		if (!Number.isFinite(min) || !Number.isFinite(max) || min < 0 || max < min) {
			return toast.error('Min/Max capacity invalid.');
		}
		creating = true;
		try {
			await registerScalableTarget({
				serviceNamespace,
				resourceId: resourceId.trim(),
				scalableDimension: scalableDimension.trim(),
				minCapacity: min,
				maxCapacity: max
			});
			toast.success('Scalable target registered.');
			resourceId = '';
			onCreated?.();
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to register target');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="max-w-md">
		<DialogHeader>
			<DialogTitle>Register scalable target</DialogTitle>
			<DialogDescription>
				Register an ECS service / Lambda alias / DynamoDB table as a scalable target.
			</DialogDescription>
		</DialogHeader>

		<div class="space-y-3">
			<div class="space-y-1.5">
				<Label for="aas-ns">Service namespace</Label>
				<select
					id="aas-ns"
					bind:value={serviceNamespace}
					class="h-9 w-full rounded-md border border-border bg-background px-3 text-sm"
				>
					{#each SERVICE_NAMESPACES as ns (ns)}
						<option value={ns}>{ns}</option>
					{/each}
				</select>
			</div>
			<div class="space-y-1.5">
				<Label for="aas-rid">Resource ID</Label>
				<Input
					id="aas-rid"
					bind:value={resourceId}
					placeholder="service/cluster-1/web"
					class="font-mono text-xs"
				/>
			</div>
			<div class="space-y-1.5">
				<Label for="aas-dim">Scalable dimension</Label>
				<Input
					id="aas-dim"
					bind:value={scalableDimension}
					placeholder="ecs:service:DesiredCount"
					class="font-mono text-xs"
				/>
			</div>
			<div class="grid grid-cols-2 gap-3">
				<div class="space-y-1.5">
					<Label for="aas-min">Min capacity</Label>
					<Input id="aas-min" bind:value={minCapacity} type="number" min="0" />
				</div>
				<div class="space-y-1.5">
					<Label for="aas-max">Max capacity</Label>
					<Input id="aas-max" bind:value={maxCapacity} type="number" min="1" />
				</div>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)} disabled={creating}>
				Cancel
			</Button>
			<Button onclick={submit} disabled={creating}>
				{creating ? 'Registering…' : 'Register target'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
