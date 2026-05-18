<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import EarIcon from '@lucide/svelte/icons/ear';
	import { toast } from 'svelte-sonner';
	import { describeListeners, type Listener, type LoadBalancer } from '$lib/api/elb';

	interface Props {
		loadBalancers: LoadBalancer[];
	}

	let { loadBalancers }: Props = $props();

	let selectedLbArn = $state<string>('');
	let listeners = $state<Listener[]>([]);
	let loading = $state(false);

	let selectedLbName = $derived(
		loadBalancers.find((lb) => lb.arn === selectedLbArn)?.name ?? ''
	);

	$effect(() => {
		if (!selectedLbArn && loadBalancers.length > 0) {
			selectedLbArn = loadBalancers[0].arn;
		}
	});

	$effect(() => {
		if (selectedLbArn) void load();
	});

	async function load() {
		if (!selectedLbArn) return;
		loading = true;
		try {
			listeners = await describeListeners(selectedLbArn);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load listeners');
			listeners = [];
		} finally {
			loading = false;
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	{#if loadBalancers.length === 0}
		<EmptyState
			icon={EarIcon}
			title="No load balancers"
			description="Create a load balancer first — listeners attach to a load balancer."
		/>
	{:else}
		<div class="flex flex-wrap items-end gap-3">
			<div class="flex flex-col gap-1">
				<Label for="elb-listener-lb">Load balancer</Label>
				<Select type="single" bind:value={selectedLbArn}>
					<SelectTrigger id="elb-listener-lb" class="min-w-[260px]">
						{selectedLbName}
					</SelectTrigger>
					<SelectContent>
						{#each loadBalancers as lb (lb.arn)}
							<SelectItem value={lb.arn} label={lb.name}>{lb.name}</SelectItem>
						{/each}
					</SelectContent>
				</Select>
			</div>
			<Button variant="ghost" size="sm" onclick={load} disabled={loading || !selectedLbArn}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
		</div>

		<DataTable
			rows={listeners}
			{loading}
			rowKey={(l) => l.arn}
			columns={[
				{ key: 'protocol', label: 'Protocol', width: '110px' },
				{ key: 'port', label: 'Port', width: '90px', align: 'right' },
				{ key: 'defaultActions', label: 'Default actions', cell: actionsCell },
				{ key: 'arn', label: 'ARN', mono: true }
			]}
		>
			{#snippet empty()}
				<EmptyState
					icon={EarIcon}
					title="No listeners"
					description="Listeners check for connection requests using the protocol and port you configure."
				/>
			{/snippet}
		</DataTable>
	{/if}
</div>

{#snippet actionsCell(l: Listener)}
	<div class="flex flex-wrap gap-1">
		{#each l.defaultActions as a (a)}
			<Badge variant="outline" class="h-4 px-1.5 text-[10px] uppercase">{a}</Badge>
		{:else}
			<span class="text-xs text-muted-foreground">—</span>
		{/each}
	</div>
{/snippet}
