<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { onMount } from 'svelte';
	import {
		listStateMachines,
		describeStateMachine,
		deleteStateMachine,
		type StateMachine,
		type StateMachineDetail,
		type Execution
	} from '$lib/api/stepfunctions';
	import { ServicePage, EmptyState } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import {
		StateMachineList,
		StateMachineHeader,
		DefinitionTab,
		ExecutionsTab,
		ExecutionDetailSheet,
		CreateStateMachineDialog
	} from '$lib/components/stepfunctions';
	import { toast } from 'svelte-sonner';
	import Plus from '@lucide/svelte/icons/plus';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Workflow from '@lucide/svelte/icons/workflow';

	let machines = $state<StateMachine[]>([]);
	let loadingList = $state(true);
	let selectedArn = $state<string | null>(null);
	let detail = $state<StateMachineDetail | null>(null);
	let detailLoading = $state(false);
	let active: string = $state(
		useTab('stepfunctions', ['definition', 'executions'] as const, 'definition', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);

	let createOpen = $state(false);
	let detailExecution = $state<Execution | null>(null);
	let detailOpen = $state(false);

	onMount(loadList);

	async function loadList() {
		loadingList = true;
		try {
			const r = await listStateMachines();
			machines = r.stateMachines;
			if (selectedArn && !machines.some((m) => m.arn === selectedArn)) {
				selectedArn = null;
				detail = null;
			}
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load state machines');
		} finally {
			loadingList = false;
		}
	}

	async function selectMachine(sm: StateMachine) {
		selectedArn = sm.arn;
		await loadDetail(sm.arn);
	}

	async function loadDetail(arn: string) {
		detailLoading = true;
		detail = null;
		try {
			detail = await describeStateMachine(arn);
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load state machine');
		} finally {
			detailLoading = false;
		}
	}

	async function handleDelete() {
		if (!detail) return;
		const name = detail.name;
		if (!confirm(`Delete state machine "${name}"?`)) return;
		try {
			await deleteStateMachine(detail.arn);
			toast.success(`Deleted ${name}`);
			selectedArn = null;
			detail = null;
			await loadList();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		}
	}

	function openExecution(exec: Execution) {
		detailExecution = exec;
		detailOpen = true;
	}

	let selectedSummary = $derived(
		machines.find((m) => m.arn === selectedArn) ?? null
	);
</script>

<ServicePage
	title="Step Functions"
	description="Coordinate distributed workflows with state machines and executions."
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={loadList} disabled={loadingList}>
			<RefreshCw />
			Refresh
		</Button>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<Plus />
			Create state machine
		</Button>
	{/snippet}

	<div
		class="grid h-full min-h-0 grid-cols-[280px_1fr] divide-x divide-border overflow-hidden"
	>
		<aside class="min-h-0 overflow-hidden">
			<StateMachineList
				stateMachines={machines}
				{selectedArn}
				loading={loadingList}
				onSelect={selectMachine}
			/>
		</aside>

		<section class="flex min-h-0 flex-col overflow-hidden">
			{#if !selectedArn}
				<div class="flex flex-1 items-center justify-center p-6">
					<EmptyState
						icon={Workflow}
						title="No state machine selected"
						description="Pick one from the list to inspect the workflow and run executions."
					/>
				</div>
			{:else if detailLoading || !detail}
				<div class="flex flex-1 items-center justify-center text-muted-foreground">
					Loading state machine...
				</div>
			{:else}
				<StateMachineHeader
					machine={detail}
					onDelete={handleDelete}
					onRefresh={() => selectedArn && loadDetail(selectedArn)}
				/>
				<Tabs bind:value={active} class="flex min-h-0 flex-1 flex-col">
					<TabsList class="mx-4 mt-2 self-start">
						<TabsTrigger value="definition">Definition</TabsTrigger>
						<TabsTrigger value="executions">Executions</TabsTrigger>
					</TabsList>
					<div class="min-h-0 flex-1 overflow-hidden">
						<TabsContent value="definition" class="m-0 h-full">
							<DefinitionTab machine={detail} loading={detailLoading} />
						</TabsContent>
						<TabsContent value="executions" class="m-0 h-full">
							<ExecutionsTab machine={selectedSummary} onSelect={openExecution} />
						</TabsContent>
					</div>
				</Tabs>
			{/if}
		</section>
	</div>
</ServicePage>

<CreateStateMachineDialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={loadList}
/>

<ExecutionDetailSheet
	execution={detailExecution}
	open={detailOpen}
	onOpenChange={(o) => (detailOpen = o)}
/>
