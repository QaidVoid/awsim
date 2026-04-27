<script lang="ts">
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import type { GraphqlApi } from '$lib/api/appsync';
	import DataSourcesTab from './data-sources-tab.svelte';
	import ResolversTab from './resolvers-tab.svelte';
	import FunctionsTab from './functions-tab.svelte';
	import SchemaTab from './schema-tab.svelte';

	interface Props {
		api: GraphqlApi;
		onDelete: () => void;
	}

	let { api, onDelete }: Props = $props();
	let activeTab = $state<'datasources' | 'resolvers' | 'functions' | 'schema'>('datasources');
</script>

<div class="flex h-full min-h-0 flex-col overflow-hidden">
	<header class="flex items-center justify-between gap-3 border-b border-border px-5 py-3">
		<div class="min-w-0">
			<div class="flex items-center gap-2">
				<h2 class="truncate font-mono text-sm font-medium">{api.name}</h2>
				<Badge variant="outline" class="h-4 px-1.5 text-[10px]">
					{api.authenticationType || 'NONE'}
				</Badge>
				{#if api.schemaStatus}
					<Badge variant="secondary" class="h-4 px-1.5 text-[10px]">{api.schemaStatus}</Badge>
				{/if}
			</div>
			<p class="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">{api.apiId}</p>
			{#if api.uris?.GRAPHQL}
				<p class="mt-0.5 truncate font-mono text-[10px] text-muted-foreground/80">
					{api.uris.GRAPHQL}
				</p>
			{/if}
		</div>
		<Button size="sm" variant="destructive" onclick={onDelete}>
			<Trash2Icon />
			Delete
		</Button>
	</header>

	<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="datasources">Data sources</TabsTrigger>
			<TabsTrigger value="resolvers">Resolvers</TabsTrigger>
			<TabsTrigger value="functions">Functions</TabsTrigger>
			<TabsTrigger value="schema">Schema</TabsTrigger>
		</TabsList>
		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="datasources" class="m-0">
				<DataSourcesTab apiId={api.apiId} />
			</TabsContent>
			<TabsContent value="resolvers" class="m-0">
				<ResolversTab apiId={api.apiId} />
			</TabsContent>
			<TabsContent value="functions" class="m-0">
				<FunctionsTab apiId={api.apiId} />
			</TabsContent>
			<TabsContent value="schema" class="m-0">
				<SchemaTab apiId={api.apiId} />
			</TabsContent>
		</div>
	</Tabs>
</div>
