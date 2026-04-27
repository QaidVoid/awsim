<script lang="ts">
	import { onMount } from 'svelte';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import {
		Tabs,
		TabsList,
		TabsTrigger,
		TabsContent,
	} from '$lib/components/ui/tabs';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import { toast } from 'svelte-sonner';
	import {
		describeLoadBalancers,
		describeTargetGroups,
		deleteLoadBalancer,
		deleteTargetGroup,
		type LoadBalancer,
		type TargetGroup,
	} from '$lib/api/elb';
	import LoadBalancersTab from '$lib/components/elb/load-balancers-tab.svelte';
	import TargetGroupsTab from '$lib/components/elb/target-groups-tab.svelte';
	import ListenersTab from '$lib/components/elb/listeners-tab.svelte';
	import RulesTab from '$lib/components/elb/rules-tab.svelte';
	import LbDetailSheet from '$lib/components/elb/lb-detail-sheet.svelte';
	import TargetGroupDetailSheet from '$lib/components/elb/target-group-detail-sheet.svelte';
	import CreateLbDialog from '$lib/components/elb/create-lb-dialog.svelte';
	import CreateTargetGroupDialog from '$lib/components/elb/create-target-group-dialog.svelte';

	let activeTab = $state<'lbs' | 'tgs' | 'listeners' | 'rules'>('lbs');

	let loadBalancers = $state<LoadBalancer[]>([]);
	let lbsLoading = $state(false);
	let targetGroups = $state<TargetGroup[]>([]);
	let tgsLoading = $state(false);

	let createLbOpen = $state(false);
	let createTgOpen = $state(false);
	let lbDetailOpen = $state(false);
	let tgDetailOpen = $state(false);
	let selectedLb = $state<LoadBalancer | null>(null);
	let selectedTg = $state<TargetGroup | null>(null);

	let confirmDeleteLb = $state<{ arn: string; name: string } | null>(null);
	let confirmDeleteTg = $state<{ arn: string; name: string } | null>(null);

	async function loadLbs() {
		lbsLoading = true;
		try {
			loadBalancers = await describeLoadBalancers();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load load balancers');
		} finally {
			lbsLoading = false;
		}
	}

	async function loadTgs() {
		tgsLoading = true;
		try {
			targetGroups = await describeTargetGroups();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load target groups');
		} finally {
			tgsLoading = false;
		}
	}

	async function refreshAll() {
		await Promise.all([loadLbs(), loadTgs()]);
	}

	function openLbSheet(lb: LoadBalancer) {
		selectedLb = lb;
		lbDetailOpen = true;
	}

	function openTgSheet(tg: TargetGroup) {
		selectedTg = tg;
		tgDetailOpen = true;
	}

	async function handleDeleteLb() {
		if (!confirmDeleteLb) return;
		const { arn, name } = confirmDeleteLb;
		confirmDeleteLb = null;
		try {
			await deleteLoadBalancer(arn);
			toast.success(`Load balancer ${name} deleted.`);
			lbDetailOpen = false;
			await loadLbs();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}

	async function handleDeleteTg() {
		if (!confirmDeleteTg) return;
		const { arn, name } = confirmDeleteTg;
		confirmDeleteTg = null;
		try {
			await deleteTargetGroup(arn);
			toast.success(`Target group ${name} deleted.`);
			tgDetailOpen = false;
			await loadTgs();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}

	onMount(refreshAll);
</script>

<ServicePage
	title="Elastic Load Balancing"
	description="Distribute traffic across targets via application, network, or gateway load balancers."
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={refreshAll} disabled={lbsLoading || tgsLoading}>
			<RefreshCwIcon class={lbsLoading || tgsLoading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	{/snippet}

	<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="lbs">Load balancers</TabsTrigger>
			<TabsTrigger value="tgs">Target groups</TabsTrigger>
			<TabsTrigger value="listeners">Listeners</TabsTrigger>
			<TabsTrigger value="rules">Rules</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="lbs" class="m-0">
				<LoadBalancersTab
					{loadBalancers}
					loading={lbsLoading}
					onRefresh={loadLbs}
					onSelect={openLbSheet}
					onCreate={() => (createLbOpen = true)}
				/>
			</TabsContent>
			<TabsContent value="tgs" class="m-0">
				<TargetGroupsTab
					{targetGroups}
					loading={tgsLoading}
					onRefresh={loadTgs}
					onSelect={openTgSheet}
					onCreate={() => (createTgOpen = true)}
				/>
			</TabsContent>
			<TabsContent value="listeners" class="m-0">
				<ListenersTab {loadBalancers} />
			</TabsContent>
			<TabsContent value="rules" class="m-0">
				<RulesTab {loadBalancers} />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<CreateLbDialog
	open={createLbOpen}
	onOpenChange={(o) => (createLbOpen = o)}
	onCreated={loadLbs}
/>

<CreateTargetGroupDialog
	open={createTgOpen}
	onOpenChange={(o) => (createTgOpen = o)}
	onCreated={loadTgs}
/>

<LbDetailSheet
	lb={selectedLb}
	open={lbDetailOpen}
	onOpenChange={(o) => (lbDetailOpen = o)}
	onDelete={(arn) => {
		const lb = loadBalancers.find((l) => l.arn === arn);
		if (lb) confirmDeleteLb = { arn, name: lb.name };
	}}
/>

<TargetGroupDetailSheet
	tg={selectedTg}
	open={tgDetailOpen}
	onOpenChange={(o) => (tgDetailOpen = o)}
	onDelete={(arn) => {
		const tg = targetGroups.find((t) => t.arn === arn);
		if (tg) confirmDeleteTg = { arn, name: tg.name };
	}}
/>

<Dialog
	open={confirmDeleteLb !== null}
	onOpenChange={(o) => {
		if (!o) confirmDeleteLb = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Delete load balancer?</DialogTitle>
			<DialogDescription>
				Permanently removes <span class="font-mono">{confirmDeleteLb?.name}</span> and all of
				its listeners.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmDeleteLb = null)}>Cancel</Button>
			<Button variant="destructive" onclick={handleDeleteLb}>Delete</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>

<Dialog
	open={confirmDeleteTg !== null}
	onOpenChange={(o) => {
		if (!o) confirmDeleteTg = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Delete target group?</DialogTitle>
			<DialogDescription>
				Permanently removes <span class="font-mono">{confirmDeleteTg?.name}</span>.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmDeleteTg = null)}>Cancel</Button>
			<Button variant="destructive" onclick={handleDeleteTg}>Delete</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
