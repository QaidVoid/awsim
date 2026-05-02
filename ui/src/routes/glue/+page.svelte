<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { ServicePage } from '$lib/components/service';
	import {
		Tabs,
		TabsList,
		TabsTrigger,
		TabsContent,
	} from '$lib/components/ui/tabs';
	import DatabasesTab from '$lib/components/glue/databases-tab.svelte';
	import TablesTab from '$lib/components/glue/tables-tab.svelte';
	import CrawlersTab from '$lib/components/glue/crawlers-tab.svelte';
	import JobsTab from '$lib/components/glue/jobs-tab.svelte';
	import ConnectionsTab from '$lib/components/glue/connections-tab.svelte';
	import DatabaseDetailSheet from '$lib/components/glue/database-detail-sheet.svelte';
	import TableDetailSheet from '$lib/components/glue/table-detail-sheet.svelte';
	import CrawlerDetailSheet from '$lib/components/glue/crawler-detail-sheet.svelte';
	import JobDetailSheet from '$lib/components/glue/job-detail-sheet.svelte';
	import ConnectionDetailSheet from '$lib/components/glue/connection-detail-sheet.svelte';
	import type {
		GlueDatabase,
		GlueTable,
		GlueCrawler,
		GlueJob,
		GlueConnection,
	} from '$lib/api/glue';

	let active: string = $state(
		useTab('glue', ['databases', 'tables', 'crawlers', 'jobs', 'connections'] as const, 'databases', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);

	let dbSheetOpen = $state(false);
	let dbName = $state<string | null>(null);

	let tableSheetOpen = $state(false);
	let tableDb = $state<string | null>(null);
	let tableName = $state<string | null>(null);

	let crawlerSheetOpen = $state(false);
	let crawlerName = $state<string | null>(null);

	let jobSheetOpen = $state(false);
	let jobName = $state<string | null>(null);

	let connSheetOpen = $state(false);
	let connName = $state<string | null>(null);

	function openDatabase(db: GlueDatabase) {
		dbName = db.name;
		dbSheetOpen = true;
	}

	function openTable(t: GlueTable) {
		tableDb = t.databaseName;
		tableName = t.name;
		tableSheetOpen = true;
	}

	function openCrawler(c: GlueCrawler) {
		crawlerName = c.name;
		crawlerSheetOpen = true;
	}

	function openJob(j: GlueJob) {
		jobName = j.name;
		jobSheetOpen = true;
	}

	function openConnection(c: GlueConnection) {
		connName = c.name;
		connSheetOpen = true;
	}
</script>

<ServicePage
	title="Glue"
	description="Managed ETL service and Data Catalog: databases, tables, crawlers, jobs, and connections."
>
	<Tabs bind:value={active} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="databases">Databases</TabsTrigger>
			<TabsTrigger value="tables">Tables</TabsTrigger>
			<TabsTrigger value="crawlers">Crawlers</TabsTrigger>
			<TabsTrigger value="jobs">Jobs</TabsTrigger>
			<TabsTrigger value="connections">Connections</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="databases" class="m-0">
				<DatabasesTab onSelect={openDatabase} />
			</TabsContent>
			<TabsContent value="tables" class="m-0">
				<TablesTab onSelect={openTable} />
			</TabsContent>
			<TabsContent value="crawlers" class="m-0">
				<CrawlersTab onSelect={openCrawler} />
			</TabsContent>
			<TabsContent value="jobs" class="m-0">
				<JobsTab onSelect={openJob} />
			</TabsContent>
			<TabsContent value="connections" class="m-0">
				<ConnectionsTab onSelect={openConnection} />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<DatabaseDetailSheet
	open={dbSheetOpen}
	name={dbName}
	onOpenChange={(o) => (dbSheetOpen = o)}
/>
<TableDetailSheet
	open={tableSheetOpen}
	databaseName={tableDb}
	name={tableName}
	onOpenChange={(o) => (tableSheetOpen = o)}
/>
<CrawlerDetailSheet
	open={crawlerSheetOpen}
	name={crawlerName}
	onOpenChange={(o) => (crawlerSheetOpen = o)}
/>
<JobDetailSheet
	open={jobSheetOpen}
	name={jobName}
	onOpenChange={(o) => (jobSheetOpen = o)}
/>
<ConnectionDetailSheet
	open={connSheetOpen}
	name={connName}
	onOpenChange={(o) => (connSheetOpen = o)}
/>
