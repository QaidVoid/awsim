<script lang="ts">
	import {
		listExecutions,
		startExecution,
		type Execution,
		type StateMachine
	} from '$lib/api/stepfunctions';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter
	} from '$lib/components/ui/dialog';
	import { toast } from 'svelte-sonner';
	import Play from '@lucide/svelte/icons/play';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Activity from '@lucide/svelte/icons/activity';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		machine: StateMachine | null;
		onSelect: (execution: Execution) => void;
	}

	let { machine, onSelect }: Props = $props();

	let executions = $state<Execution[]>([]);
	let loading = $state(false);
	let lastArn = $state('');

	let startOpen = $state(false);
	let starting = $state(false);
	let execName = $state('');
	let execInput = $state('{}');

	$effect(() => {
		if (machine && machine.arn !== lastArn) {
			lastArn = machine.arn;
			void load();
		}
		if (!machine) {
			executions = [];
			lastArn = '';
		}
	});

	async function load() {
		if (!machine) return;
		loading = true;
		try {
			const r = await listExecutions(machine.arn);
			executions = r.executions;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load executions');
		} finally {
			loading = false;
		}
	}

	async function start(e: Event) {
		e.preventDefault();
		if (!machine) return;
		starting = true;
		try {
			const r = await startExecution(
				machine.arn,
				execInput || '{}',
				execName.trim() || undefined
			);
			toast.success('Execution started');
			const name = execName.trim();
			startOpen = false;
			execName = '';
			execInput = '{}';
			await load();
			// Open the just-started execution so you watch it run
			// instead of hunting for it in the list.
			onSelect({
				arn: r.executionArn,
				name,
				stateMachineArn: machine.arn,
				status: 'RUNNING',
				startDate: r.startDate
			});
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to start');
		} finally {
			starting = false;
		}
	}

	function statusVariant(s: string): 'default' | 'secondary' | 'destructive' | 'outline' {
		if (s === 'SUCCEEDED') return 'default';
		if (s === 'FAILED' || s === 'TIMED_OUT' || s === 'ABORTED') return 'destructive';
		if (s === 'RUNNING') return 'secondary';
		return 'outline';
	}

	function shortArn(arn: string): string {
		return arn.split(':').pop() ?? arn;
	}

	function formatDate(iso?: string): string {
		if (!iso) return '—';
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header
		class="flex items-center justify-between border-b border-border bg-background/40 px-4 py-2"
	>
		<div class="text-xs text-muted-foreground">
			{machine ? `${executions.length} execution${executions.length === 1 ? '' : 's'}` : 'No machine selected'}
		</div>
		<div class="flex items-center gap-2">
			<Button type="button" variant="outline" size="sm" onclick={load} disabled={loading || !machine}>
				<RefreshCw />
				Refresh
			</Button>
			<Button
				type="button"
				size="sm"
				onclick={() => (startOpen = true)}
				disabled={!machine}
			>
				<Play />
				Start execution
			</Button>
		</div>
	</header>

	<div class="min-h-0 flex-1 overflow-y-auto">
		{#if !machine}
			<div class="p-6">
				<EmptyState
					icon={Activity}
					title="No state machine selected"
					description="Pick a machine from the list to view executions."
				/>
			</div>
		{:else if loading && executions.length === 0}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if executions.length === 0}
			<div class="p-6">
				<EmptyState
					icon={Activity}
					title="No executions yet"
					description="Click Start execution to run this machine."
				/>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="border-b border-border bg-background/95 text-left text-muted-foreground">
					<tr>
						<th class="px-4 py-2 font-medium">Name</th>
						<th class="px-4 py-2 font-medium">Status</th>
						<th class="px-4 py-2 font-medium">Started</th>
						<th class="px-4 py-2 font-medium">Stopped</th>
					</tr>
				</thead>
				<tbody>
					{#each executions as ex (ex.arn)}
						<tr
							class="cursor-pointer border-b border-border/40 hover:bg-muted/40"
							onclick={() => onSelect(ex)}
						>
							<td class="px-4 py-2 font-mono text-xs">{ex.name || shortArn(ex.arn)}</td>
							<td class="px-4 py-2"><Badge variant={statusVariant(ex.status)}>{ex.status}</Badge></td>
							<td class="px-4 py-2 text-xs text-muted-foreground">{formatDate(ex.startDate)}</td>
							<td class="px-4 py-2 text-xs text-muted-foreground">{formatDate(ex.stopDate)}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>

<Dialog open={startOpen} onOpenChange={(o) => (startOpen = o)}>
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle>Start execution</DialogTitle>
			<DialogDescription>
				Optional name + input JSON for {machine?.name ?? 'state machine'}.
			</DialogDescription>
		</DialogHeader>
		<form onsubmit={start} class="flex flex-col gap-3 py-2">
			<div class="flex flex-col gap-1.5">
				<Label for="exec-name">Execution name (optional)</Label>
				<Input id="exec-name" bind:value={execName} placeholder="run-1" />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="exec-input">Input JSON</Label>
				<textarea
					id="exec-input"
					bind:value={execInput}
					rows="8"
					spellcheck="false"
					class="resize-y rounded-md border border-border bg-background p-3 font-mono text-xs outline-none focus:border-ring focus:ring-1 focus:ring-ring"
				></textarea>
			</div>
			<DialogFooter>
				<Button type="button" variant="ghost" onclick={() => (startOpen = false)}>Cancel</Button>
				<Button type="submit" disabled={starting}>
					<Play />
					{starting ? 'Starting...' : 'Start'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
