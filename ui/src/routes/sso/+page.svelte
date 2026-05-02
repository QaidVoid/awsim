<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import InstancesTab from '$lib/components/sso/instances-tab.svelte';
	import PermissionSetsTab from '$lib/components/sso/permission-sets-tab.svelte';
	import AccountAssignmentsTab from '$lib/components/sso/account-assignments-tab.svelte';
	import type { Instance } from '$lib/api/sso-admin';

	let active: string = $state(
		useTab('sso', ['instances', 'permission-sets', 'assignments'] as const, 'instances', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);
	let instances = $state<Instance[]>([]);

	const selectedInstance = $derived(instances[0] ?? null);

	function onLoaded(list: Instance[]) {
		instances = list;
	}
</script>

<ServicePage
	title="SSO Admin"
	description="IAM Identity Center instances, permission sets, and account assignments."
>
	<Tabs bind:value={active} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="instances">Instances</TabsTrigger>
			<TabsTrigger value="permission-sets">Permission sets</TabsTrigger>
			<TabsTrigger value="assignments">Account assignments</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="instances" class="m-0">
				<InstancesTab {instances} {onLoaded} />
			</TabsContent>
			<TabsContent value="permission-sets" class="m-0">
				<PermissionSetsTab instance={selectedInstance} />
			</TabsContent>
			<TabsContent value="assignments" class="m-0">
				<AccountAssignmentsTab instance={selectedInstance} />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>
