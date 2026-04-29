<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import ClustersTab from '$lib/components/memorydb/clusters-tab.svelte';
	import UsersTab from '$lib/components/memorydb/users-tab.svelte';
	import AclsTab from '$lib/components/memorydb/acls-tab.svelte';

	let activeTab = $state<'clusters' | 'users' | 'acls'>('clusters');
	let refreshKey = $state(0);

	function refresh() {
		refreshKey += 1;
	}
</script>

<ServicePage
	title="MemoryDB"
	description="Redis-compatible MemoryDB clusters, users, and ACLs."
>
	<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="clusters">Clusters</TabsTrigger>
			<TabsTrigger value="users">Users</TabsTrigger>
			<TabsTrigger value="acls">ACLs</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="clusters" class="m-0">
				<ClustersTab {refreshKey} onChanged={refresh} />
			</TabsContent>
			<TabsContent value="users" class="m-0">
				<UsersTab {refreshKey} onChanged={refresh} />
			</TabsContent>
			<TabsContent value="acls" class="m-0">
				<AclsTab {refreshKey} onChanged={refresh} />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>
