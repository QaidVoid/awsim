<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import { toast } from 'svelte-sonner';
	import {
		describeListeners,
		describeRules,
		type Listener,
		type LoadBalancer,
		type Rule
	} from '$lib/api/elb';

	interface Props {
		loadBalancers: LoadBalancer[];
	}

	let { loadBalancers }: Props = $props();

	let selectedLbArn = $state<string>('');
	let selectedListenerArn = $state<string>('');
	let listeners = $state<Listener[]>([]);
	let rules = $state<Rule[]>([]);
	let loadingListeners = $state(false);
	let loadingRules = $state(false);

	let selectedLbName = $derived(
		loadBalancers.find((lb) => lb.arn === selectedLbArn)?.name ?? ''
	);
	let selectedListenerLabel = $derived.by(() => {
		const l = listeners.find((x) => x.arn === selectedListenerArn);
		return l ? `${l.protocol}:${l.port}` : '';
	});

	$effect(() => {
		if (!selectedLbArn && loadBalancers.length > 0) {
			selectedLbArn = loadBalancers[0].arn;
		}
	});

	$effect(() => {
		if (selectedLbArn) void loadListeners();
	});

	$effect(() => {
		if (selectedListenerArn) void loadRules();
	});

	async function loadListeners() {
		loadingListeners = true;
		selectedListenerArn = '';
		rules = [];
		try {
			listeners = await describeListeners(selectedLbArn);
			if (listeners.length > 0) selectedListenerArn = listeners[0].arn;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load listeners');
			listeners = [];
		} finally {
			loadingListeners = false;
		}
	}

	async function loadRules() {
		loadingRules = true;
		try {
			rules = await describeRules(selectedListenerArn);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load rules');
			rules = [];
		} finally {
			loadingRules = false;
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	{#if loadBalancers.length === 0}
		<EmptyState
			icon={GitBranchIcon}
			title="No load balancers"
			description="Create a load balancer and listener first — rules attach to listeners."
		/>
	{:else}
		<div class="flex flex-wrap items-end gap-3">
			<div class="flex flex-col gap-1">
				<Label for="elb-rules-lb">Load balancer</Label>
				<Select type="single" bind:value={selectedLbArn}>
					<SelectTrigger id="elb-rules-lb" class="min-w-[260px]">
						{selectedLbName}
					</SelectTrigger>
					<SelectContent>
						{#each loadBalancers as lb (lb.arn)}
							<SelectItem value={lb.arn} label={lb.name}>{lb.name}</SelectItem>
						{/each}
					</SelectContent>
				</Select>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="elb-rules-listener">Listener</Label>
				<Select
					type="single"
					bind:value={selectedListenerArn}
					disabled={loadingListeners || listeners.length === 0}
				>
					<SelectTrigger id="elb-rules-listener" class="min-w-[260px]">
						{selectedListenerArn ? selectedListenerLabel : 'No listeners'}
					</SelectTrigger>
					<SelectContent>
						{#each listeners as l (l.arn)}
							<SelectItem value={l.arn} label={`${l.protocol}:${l.port}`}
								>{l.protocol}:{l.port}</SelectItem
							>
						{/each}
					</SelectContent>
				</Select>
			</div>
			<Button
				variant="ghost"
				size="sm"
				onclick={loadRules}
				disabled={loadingRules || !selectedListenerArn}
			>
				<RefreshCwIcon class={loadingRules ? 'animate-spin' : ''} />
				Refresh
			</Button>
		</div>

		<DataTable
			rows={rules}
			loading={loadingRules}
			rowKey={(r) => r.arn || r.priority}
			columns={[
				{ key: 'priority', label: 'Priority', width: '110px', cell: priorityCell },
				{ key: 'conditions', label: 'Conditions', cell: condCell },
				{ key: 'actions', label: 'Actions', width: '180px', cell: actionsCell }
			]}
		>
			{#snippet empty()}
				<EmptyState
					icon={GitBranchIcon}
					title="No rules"
					description="Listener rules forward, redirect, or authenticate based on conditions like host or path."
				/>
			{/snippet}
		</DataTable>
	{/if}
</div>

{#snippet priorityCell(r: Rule)}
	{#if r.isDefault}
		<Badge variant="outline" class="h-5 px-2 text-[10px]">DEFAULT</Badge>
	{:else}
		<span class="font-mono text-xs">{r.priority}</span>
	{/if}
{/snippet}

{#snippet condCell(r: Rule)}
	{#if r.conditions.length === 0}
		<span class="text-xs text-muted-foreground">always</span>
	{:else}
		<div class="flex flex-col gap-0.5 text-[11px]">
			{#each r.conditions as c (c.field)}
				<span class="font-mono">
					<span class="text-muted-foreground">{c.field}:</span>
					{c.values.join(', ')}
				</span>
			{/each}
		</div>
	{/if}
{/snippet}

{#snippet actionsCell(r: Rule)}
	<div class="flex flex-wrap gap-1">
		{#each r.actions as a (a)}
			<Badge variant="outline" class="h-4 px-1.5 text-[10px] uppercase">{a}</Badge>
		{/each}
	</div>
{/snippet}
