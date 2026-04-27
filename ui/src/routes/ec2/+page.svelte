<script lang="ts">
	import { onMount } from 'svelte';
	import {
		describeInstances,
		describeSecurityGroups,
		describeKeyPairs,
		describeVpcs,
		describeSubnets,
		describeVolumes,
		type Instance,
		type SecurityGroup,
		type KeyPair,
		type Vpc,
		type Subnet,
		type Volume
	} from '$lib/api/ec2';
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import {
		InstancesTab,
		SecurityGroupsTab,
		KeyPairsTab,
		VpcsTab,
		SubnetsTab,
		VolumesTab,
		InstanceDetailSheet,
		LaunchInstanceDialog
	} from '$lib/components/ec2';
	import { toast } from 'svelte-sonner';

	let activeTab = $state<'instances' | 'security' | 'keys' | 'vpcs' | 'subnets' | 'volumes'>(
		'instances'
	);

	let instances = $state<Instance[]>([]);
	let groups = $state<SecurityGroup[]>([]);
	let keys = $state<KeyPair[]>([]);
	let vpcs = $state<Vpc[]>([]);
	let subnets = $state<Subnet[]>([]);
	let volumes = $state<Volume[]>([]);

	let loadingInstances = $state(false);
	let loadingGroups = $state(false);
	let loadingKeys = $state(false);
	let loadingVpcs = $state(false);
	let loadingSubnets = $state(false);
	let loadingVolumes = $state(false);

	let detailInstance = $state<Instance | null>(null);
	let detailOpen = $state(false);
	let launchOpen = $state(false);

	onMount(() => {
		void loadInstances();
		void loadVpcs();
		void loadSubnets();
		void loadKeys();
		void loadGroups();
	});

	async function loadInstances() {
		loadingInstances = true;
		try {
			instances = await describeInstances();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load instances');
		} finally {
			loadingInstances = false;
		}
	}

	async function loadGroups() {
		loadingGroups = true;
		try {
			groups = await describeSecurityGroups();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load security groups');
		} finally {
			loadingGroups = false;
		}
	}

	async function loadKeys() {
		loadingKeys = true;
		try {
			keys = await describeKeyPairs();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load key pairs');
		} finally {
			loadingKeys = false;
		}
	}

	async function loadVpcs() {
		loadingVpcs = true;
		try {
			vpcs = await describeVpcs();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load VPCs');
		} finally {
			loadingVpcs = false;
		}
	}

	async function loadSubnets() {
		loadingSubnets = true;
		try {
			subnets = await describeSubnets();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load subnets');
		} finally {
			loadingSubnets = false;
		}
	}

	async function loadVolumes() {
		loadingVolumes = true;
		try {
			volumes = await describeVolumes();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load volumes');
		} finally {
			loadingVolumes = false;
		}
	}

	function openInstance(i: Instance) {
		detailInstance = i;
		detailOpen = true;
	}
</script>

<ServicePage
	title="EC2"
	description="Elastic Compute Cloud — instances, networking, security, keys, and storage."
>
	<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-col">
		<TabsList class="mx-4 mt-2 self-start">
			<TabsTrigger value="instances">Instances</TabsTrigger>
			<TabsTrigger value="security">Security groups</TabsTrigger>
			<TabsTrigger value="keys">Key pairs</TabsTrigger>
			<TabsTrigger value="vpcs">VPCs</TabsTrigger>
			<TabsTrigger value="subnets">Subnets</TabsTrigger>
			<TabsTrigger value="volumes">Volumes</TabsTrigger>
		</TabsList>
		<div class="min-h-0 flex-1">
			<TabsContent value="instances" class="m-0 h-full">
				<InstancesTab
					{instances}
					loading={loadingInstances}
					onReload={loadInstances}
					onSelect={openInstance}
					onLaunch={() => (launchOpen = true)}
				/>
			</TabsContent>
			<TabsContent value="security" class="m-0 h-full">
				<SecurityGroupsTab {groups} {vpcs} loading={loadingGroups} onReload={loadGroups} />
			</TabsContent>
			<TabsContent value="keys" class="m-0 h-full">
				<KeyPairsTab {keys} loading={loadingKeys} onReload={loadKeys} />
			</TabsContent>
			<TabsContent value="vpcs" class="m-0 h-full">
				<VpcsTab {vpcs} loading={loadingVpcs} onReload={loadVpcs} />
			</TabsContent>
			<TabsContent value="subnets" class="m-0 h-full">
				<SubnetsTab {subnets} loading={loadingSubnets} onReload={loadSubnets} />
			</TabsContent>
			<TabsContent value="volumes" class="m-0 h-full">
				<VolumesTab {volumes} loading={loadingVolumes} onReload={loadVolumes} />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<InstanceDetailSheet
	instance={detailInstance}
	open={detailOpen}
	onOpenChange={(o) => (detailOpen = o)}
/>

<LaunchInstanceDialog
	open={launchOpen}
	{subnets}
	{keys}
	{groups}
	onOpenChange={(o) => (launchOpen = o)}
	onLaunched={loadInstances}
/>
