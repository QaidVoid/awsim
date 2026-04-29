<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import { Input } from '$lib/components/ui/input';
	import UsersTab from '$lib/components/identitystore/users-tab.svelte';
	import GroupsTab from '$lib/components/identitystore/groups-tab.svelte';

	let identityStoreId = $state('d-1234567890');
	let activeTab = $state<'users' | 'groups'>('users');
	let refreshKey = $state(0);

	function refresh() {
		refreshKey += 1;
	}
</script>

<ServicePage
	title="Identity Store"
	description="Users, groups, and group memberships scoped by IdentityStoreId. Pairs with IAM Identity Center."
>
	{#snippet actions()}
		<div class="flex items-center gap-2">
			<label for="ids-id" class="text-xs text-muted-foreground">Identity Store</label>
			<Input
				id="ids-id"
				bind:value={identityStoreId}
				class="h-7 max-w-[180px] font-mono text-xs"
			/>
		</div>
	{/snippet}

	<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="users">Users</TabsTrigger>
			<TabsTrigger value="groups">Groups</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="users" class="m-0">
				<UsersTab {identityStoreId} {refreshKey} onChanged={refresh} />
			</TabsContent>
			<TabsContent value="groups" class="m-0">
				<GroupsTab {identityStoreId} {refreshKey} onChanged={refresh} />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>
