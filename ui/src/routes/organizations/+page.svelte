<script lang="ts">
	/**
	 * Organizations page — accounts, OUs, SCPs, and roots.
	 */
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsList, TabsTrigger, TabsContent } from '$lib/components/ui/tabs';
	import {
		AccountsTab,
		OusTab,
		PoliciesTab,
		RootsTab,
		ScpEditor,
		AccountDetailSheet
	} from '$lib/components/organizations';
	import type { Account, Policy } from '$lib/api/organizations';

	let activeTab = $state<'accounts' | 'ous' | 'policies' | 'roots'>('accounts');

	let selectedAccount = $state<Account | null>(null);
	let accountOpen = $state(false);

	let selectedPolicy = $state<Policy | null>(null);
	let policyOpen = $state(false);

	function selectAccount(a: Account) {
		selectedAccount = a;
		accountOpen = true;
	}

	function selectPolicy(p: Policy) {
		selectedPolicy = p;
		policyOpen = true;
	}
</script>

<svelte:head>
	<title>AWSim · Organizations</title>
</svelte:head>

<ServicePage
	title="Organizations"
	description="Centrally manage accounts, OUs, and service control policies."
>
	<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-col">
		<TabsList class="mx-4 mt-2 self-start">
			<TabsTrigger value="accounts">Accounts</TabsTrigger>
			<TabsTrigger value="ous">OUs</TabsTrigger>
			<TabsTrigger value="policies">SCPs</TabsTrigger>
			<TabsTrigger value="roots">Roots</TabsTrigger>
		</TabsList>
		<div class="min-h-0 flex-1">
			<TabsContent value="accounts" class="m-0 h-full">
				<AccountsTab onSelect={selectAccount} />
			</TabsContent>
			<TabsContent value="ous" class="m-0 h-full">
				<OusTab />
			</TabsContent>
			<TabsContent value="policies" class="m-0 h-full">
				<PoliciesTab onSelect={selectPolicy} />
			</TabsContent>
			<TabsContent value="roots" class="m-0 h-full">
				<RootsTab />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>

<AccountDetailSheet
	account={selectedAccount}
	open={accountOpen}
	onOpenChange={(o) => (accountOpen = o)}
/>
<ScpEditor policy={selectedPolicy} open={policyOpen} onOpenChange={(o) => (policyOpen = o)} />
