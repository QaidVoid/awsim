<script lang="ts">
	/**
	 * CloudWatch Metrics page — tabbed view of metrics, alarms and dashboards.
	 */
	import { ServicePage } from '$lib/components/service';
	import {
		Tabs,
		TabsContent,
		TabsList,
		TabsTrigger,
	} from '$lib/components/ui/tabs';
	import MetricsTab from '$lib/components/cloudwatch-metrics/metrics-tab.svelte';
	import AlarmsTab from '$lib/components/cloudwatch-metrics/alarms-tab.svelte';
	import DashboardsTab from '$lib/components/cloudwatch-metrics/dashboards-tab.svelte';

	let tab = $state<'metrics' | 'alarms' | 'dashboards'>('metrics');
</script>

<svelte:head>
	<title>AWSim · CloudWatch Metrics</title>
</svelte:head>

<ServicePage title="CloudWatch Metrics" description="Metrics, alarms and dashboards.">
	<Tabs
		value={tab}
		onValueChange={(v) => (tab = v as 'metrics' | 'alarms' | 'dashboards')}
		class="flex h-full min-h-0 flex-col"
	>
		<div class="border-b border-border px-4 pt-3">
			<TabsList>
				<TabsTrigger value="metrics">Metrics</TabsTrigger>
				<TabsTrigger value="alarms">Alarms</TabsTrigger>
				<TabsTrigger value="dashboards">Dashboards</TabsTrigger>
			</TabsList>
		</div>
		<TabsContent value="metrics" class="min-h-0 flex-1">
			<MetricsTab />
		</TabsContent>
		<TabsContent value="alarms" class="min-h-0 flex-1">
			<AlarmsTab />
		</TabsContent>
		<TabsContent value="dashboards" class="min-h-0 flex-1">
			<DashboardsTab />
		</TabsContent>
	</Tabs>
</ServicePage>
