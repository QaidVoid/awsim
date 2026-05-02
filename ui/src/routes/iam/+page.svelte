<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import UsersTab from '$lib/components/iam/users-tab.svelte';
	import RolesTab from '$lib/components/iam/roles-tab.svelte';
	import GroupsTab from '$lib/components/iam/groups-tab.svelte';
	import PoliciesTab from '$lib/components/iam/policies-tab.svelte';
	import SimulatorTab from '$lib/components/iam/simulator-tab.svelte';
	import InstanceProfilesTab from '$lib/components/iam/instance-profiles-tab.svelte';

	let active = $state('users');
</script>

<ServicePage
	title="IAM"
	description="Identity and Access Management — users, roles, groups, policies, and a working policy simulator."
>
	<Tabs bind:value={active} class="flex h-full min-h-0 flex-col">
		<div class="border-b border-border px-6 pt-3">
			<TabsList variant="line">
				<TabsTrigger value="users">Users</TabsTrigger>
				<TabsTrigger value="roles">Roles</TabsTrigger>
				<TabsTrigger value="groups">Groups</TabsTrigger>
			<TabsTrigger value="policies">Policies</TabsTrigger>
			<TabsTrigger value="instance-profiles">Instance Profiles</TabsTrigger>
			<TabsTrigger value="simulator">Simulator</TabsTrigger>
			</TabsList>
		</div>

		<TabsContent value="users" class="min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden">
			<UsersTab />
		</TabsContent>
		<TabsContent value="roles" class="min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden">
			<RolesTab />
		</TabsContent>
		<TabsContent
			value="groups"
			class="min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
		>
			<GroupsTab />
		</TabsContent>
		<TabsContent
			value="policies"
			class="min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
		>
			<PoliciesTab />
		</TabsContent>
		<TabsContent
			value="instance-profiles"
			class="min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
		>
			<InstanceProfilesTab />
		</TabsContent>
		<TabsContent
			value="simulator"
			class="min-h-0 flex-1 overflow-y-auto data-[state=inactive]:hidden"
		>
			<SimulatorTab />
		</TabsContent>
	</Tabs>
</ServicePage>
