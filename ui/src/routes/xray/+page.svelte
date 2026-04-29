<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import TracesTab from '$lib/components/xray/traces-tab.svelte';
	import ServiceGraphTab from '$lib/components/xray/service-graph-tab.svelte';
	import TraceDetailSheet from '$lib/components/xray/trace-detail-sheet.svelte';
	import type { TraceSummary } from '$lib/api/xray';

	let activeTab = $state<'traces' | 'graph'>('traces');
	let detailOpen = $state(false);
	let detailSummary = $state<TraceSummary | null>(null);

	function openDetail(s: TraceSummary) {
		detailSummary = s;
		detailOpen = true;
	}
</script>

<ServicePage
	title="X-Ray"
	description="Distributed traces and service graph for instrumented workloads."
>
	<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="traces">Traces</TabsTrigger>
			<TabsTrigger value="graph">Service graph</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="traces" class="m-0">
				<TracesTab onSelect={openDetail} />
			</TabsContent>
			<TabsContent value="graph" class="m-0">
				<ServiceGraphTab />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<TraceDetailSheet
	open={detailOpen}
	summary={detailSummary}
	onOpenChange={(o) => (detailOpen = o)}
/>
