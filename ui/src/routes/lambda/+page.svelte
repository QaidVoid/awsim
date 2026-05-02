<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { onMount } from 'svelte';
	import {
		listFunctions,
		getFunction,
		deleteFunction,
		type LambdaFunction,
		type LambdaFunctionDetail
	} from '$lib/api/lambda';
	import { ServicePage, EmptyState } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import {
		FunctionList,
		FunctionHeader,
		CodeTab,
		ConfigTab,
		ConcurrencyTab,
		EventSourcesTab,
		InvokeTab,
		VersionsTab,
		LogsTab,
		CreateFunctionDialog
	} from '$lib/components/lambda';
	import { toast } from 'svelte-sonner';
	import Plus from '@lucide/svelte/icons/plus';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Zap from '@lucide/svelte/icons/zap';

	const RUNTIMES = [
		'python3.12',
		'python3.11',
		'python3.10',
		'nodejs20.x',
		'nodejs18.x',
		'java21',
		'java17',
		'go1.x',
		'dotnet8',
		'ruby3.3',
		'provided.al2023'
	];

	let functions = $state<LambdaFunction[]>([]);
	let loadingList = $state(true);
	let selectedName = $state<string | null>(null);
	let detail = $state<LambdaFunctionDetail | null>(null);
	let detailLoading = $state(false);
	let active: string = $state(
		useTab('lambda', ['invoke', 'config', 'code', 'versions', 'concurrency', 'sources', 'logs'] as const, 'invoke', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);
	let createOpen = $state(false);

	onMount(loadList);

	async function loadList() {
		loadingList = true;
		try {
			const r = await listFunctions();
			functions = r.functions;
			if (selectedName && !functions.some((f) => f.name === selectedName)) {
				selectedName = null;
				detail = null;
			}
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load functions');
		} finally {
			loadingList = false;
		}
	}

	async function selectFn(fn: LambdaFunction) {
		selectedName = fn.name;
		await loadDetail(fn.name);
	}

	async function loadDetail(name: string) {
		detailLoading = true;
		detail = null;
		try {
			detail = await getFunction(name);
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load function detail');
		} finally {
			detailLoading = false;
		}
	}

	async function handleDelete() {
		if (!selectedName) return;
		const name = selectedName;
		if (!confirm(`Delete function "${name}"?`)) return;
		try {
			await deleteFunction(name);
			toast.success(`Deleted ${name}`);
			selectedName = null;
			detail = null;
			await loadList();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Delete failed');
		}
	}

	async function handleCreated(name: string) {
		await loadList();
		const fn = functions.find((f) => f.name === name);
		if (fn) await selectFn(fn);
	}
</script>

<ServicePage
	title="Lambda"
	description="Serverless functions — invoke, edit configuration, publish versions, tail logs."
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={loadList} disabled={loadingList}>
			<RefreshCw />
			Refresh
		</Button>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<Plus />
			Create function
		</Button>
	{/snippet}

	<div
		class="grid h-full min-h-0 grid-cols-[280px_1fr] divide-x divide-border overflow-hidden"
	>
		<aside class="min-h-0 overflow-hidden">
			<FunctionList
				{functions}
				{selectedName}
				loading={loadingList}
				onSelect={selectFn}
			/>
		</aside>

		<section class="flex min-h-0 flex-col overflow-hidden">
			{#if !selectedName}
				<div class="flex flex-1 items-center justify-center p-6">
					<EmptyState
						icon={Zap}
						title="No function selected"
						description="Choose a function from the list to invoke, configure, or inspect logs."
					/>
				</div>
			{:else if detailLoading || !detail}
				<div class="flex flex-1 items-center justify-center text-muted-foreground">
					Loading function...
				</div>
			{:else}
				<FunctionHeader
					config={detail.configuration}
					onDelete={handleDelete}
					onRefresh={() => selectedName && loadDetail(selectedName)}
				/>
				<Tabs bind:value={active} class="flex min-h-0 flex-1 flex-col">
					<TabsList class="mx-4 mt-2 self-start">
						<TabsTrigger value="invoke">Invoke</TabsTrigger>
						<TabsTrigger value="config">Configuration</TabsTrigger>
						<TabsTrigger value="code">Code</TabsTrigger>
						<TabsTrigger value="versions">Versions</TabsTrigger>
						<TabsTrigger value="concurrency">Concurrency</TabsTrigger>
						<TabsTrigger value="sources">Event sources</TabsTrigger>
						<TabsTrigger value="logs">Logs</TabsTrigger>
					</TabsList>
					<div class="min-h-0 flex-1 overflow-y-auto">
						<TabsContent value="invoke" class="m-0">
							<InvokeTab functionName={selectedName} />
						</TabsContent>
						<TabsContent value="config" class="m-0">
							<ConfigTab
								config={detail.configuration}
								runtimes={RUNTIMES}
								onSaved={(next) => {
									if (detail) detail = { ...detail, configuration: next };
								}}
							/>
						</TabsContent>
						<TabsContent value="code" class="m-0">
							<CodeTab {detail} loading={detailLoading} />
						</TabsContent>
						<TabsContent value="versions" class="m-0 h-full">
							<VersionsTab functionName={selectedName} />
						</TabsContent>
						<TabsContent value="concurrency" class="m-0 h-full">
							<ConcurrencyTab functionName={selectedName} />
						</TabsContent>
						<TabsContent value="sources" class="m-0 h-full">
							<EventSourcesTab functionName={selectedName} />
						</TabsContent>
						<TabsContent value="logs" class="m-0 h-full">
							<LogsTab functionName={selectedName} />
						</TabsContent>
					</div>
				</Tabs>
			{/if}
		</section>
	</div>
</ServicePage>

<CreateFunctionDialog
	open={createOpen}
	runtimes={RUNTIMES}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={handleCreated}
/>
